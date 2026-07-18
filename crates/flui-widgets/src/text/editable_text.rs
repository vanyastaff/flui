//! [`EditableText`] — single-line editable text backed by a
//! [`TextEditingController`].

use std::{
    cell::{Cell, RefCell},
    ops::Range,
    rc::Rc,
    sync::Arc,
};

use flui_foundation::ListenerId;
use flui_foundation::notifier::Listenable;
use flui_interaction::events::{Key, KeyState, NamedKey};
use flui_interaction::routing::{FocusManager, FocusNode, KeyEventCallback};
use flui_interaction::{ClientToken, TextInputHandle};
use flui_objects::RenderEditable;
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::protocol::BoxProtocol;
use flui_types::{
    Color, ImeEvent, Point, Rect,
    geometry::{Bounds, Pixels},
    typography::{TextDirection, TextSpan, TextStyle},
};
use flui_view::prelude::*;
use flui_view::{BoxedView, RenderView, impl_render_view};
use parking_lot::RwLock;

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
/// # IME composition
///
/// On focus gain, `EditableTextState` attaches an IME client through
/// [`BuildContext::text_input_handle`] (acquired in `init_state`, per the
/// frame-capability rule that method's doc states) — its callback routes
/// each [`ImeEvent`] to the matching [`TextEditingController`] composing
/// operation (`Preedit` → `set_composing_text`, `Commit` → `commit_text`,
/// `Disabled` → `clear_composing`). On blur and on dispose the client is
/// detached (the ADR-0030 detach-on-dispose contract — a field unmounted
/// while still focused must not leave a stale IME client attached).
///
/// **Suppression contract**: the key handler skips `Key::Character`
/// insertion only while [`TextEditingController::is_composing`] is `true` —
/// suppressing unconditionally after focus gain would silently kill plain
/// (non-IME) typing for the rest of the session, since winit only sends
/// `Key::Character` for keys it did **not** already route through
/// composition. See [`ImeEvent`]'s doc for the full contract.
///
/// # IME cursor-area tracking
///
/// While an IME client is attached (focus gain to blur/dispose),
/// `EditableTextState` also runs a self-rescheduling post-frame loop (ADR-0032)
/// that reads the composing region's current global rect when one is
/// active, falling back to the collapsed caret's rect otherwise — through
/// the second, inner [`SubtreeAnchor`](flui_objects::SubtreeAnchor) wrapping
/// the render view directly (`build_field_view`) and
/// [`RenderEditable::rect_for_composing_range`]/
/// [`RenderEditable::caret_local_rect`] — and forwards it to
/// [`TextInputHandle::set_cursor_area`] whenever it changes, so the platform
/// IME candidate window follows the composing text (or the caret, once
/// composition ends). This is a winit single-rect reduction of Flutter's
/// transform+local-rect protocol (`editable_text.dart`'s
/// `_updateSizeAndTransform`/`_updateComposingRectIfNeeded`/
/// `_schedulePeriodicPostFrameCallbacks`, tag `3.44.0`) — see ADR-0032 for
/// the loop mechanics (why it is per-attach: a fresh alive-flag and a fresh
/// last-sent cache each attach, rather than shared across the field's
/// lifetime) and ADR-0033 for the composing-rect-over-caret-rect fallback
/// order this loop now applies.
///
/// # DEFERRED (v1)
///
/// The following are absent in v1; do not use these features and expect them
/// to work:
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
    /// (`focus_next`/`focus_previous`) already honors and which — matching
    /// Flutter's `FocusNode.canRequestFocus` setter — releases primary focus
    /// itself if the field is focused when it becomes disabled; no separate
    /// `did_update_view` unfocus step is needed. Its key handler also stops
    /// mutating the controller while disabled, so even a stray dispatch
    /// reaching an already-focused-then-disabled node is a no-op.
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
    /// Publishes the `RenderId` of exactly the `EditableTextRenderView` —
    /// the inner anchor (ADR-0032), wrapped directly around it in
    /// `build_field_view`, so the IME cursor-area loop's `transform_to`
    /// starts right at the editable instead of walking through `anchor`'s
    /// wider subtree (which also covers the `AnimatedBuilder` in between).
    inner_anchor: flui_objects::SubtreeAnchor,
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
    /// ID for the second `FocusManager` listener — attaches/detaches the IME
    /// client on this field's own focus transitions. Kept separate from
    /// `focus_listener_id` so the (already-tested) rebuild-on-focus-change
    /// listener is untouched by the IME wiring.
    ime_focus_listener_id: Option<ListenerId>,
    /// The IME attach/detach capability, acquired once in `init_state` (the
    /// frame-capability rule `post_frame_handle` follows —
    /// `BuildContext::text_input_handle`'s doc). `None` when no binding
    /// installed one (a bare `ElementTree` in a unit test): the field then
    /// simply never attaches, rather than panicking or silently no-opping
    /// through a stub.
    ime_handle: Option<TextInputHandle>,
    /// The active IME client token, if this field currently has one
    /// attached. Shared with the IME focus-listener closure (`Rc<RefCell<_>>`
    /// because that closure is `'static` and cannot borrow `&mut self`) so
    /// both the closure (attach on focus gain, detach on blur) and `dispose`
    /// (detach-on-unmount, independent of any focus-loss notification) can
    /// clear it.
    ime_token: Rc<RefCell<Option<ClientToken>>>,
    /// The post-frame scheduling capability the IME cursor-area loop uses,
    /// acquired once in `init_state` beside `ime_handle` (trigger-22: a
    /// lifecycle-only frame capability is acquired in `init_state`, never in
    /// `build`). `None` under a binding that installs no post-frame handle —
    /// the loop then simply never starts (warned, not panicked; see
    /// `init_state`'s IME focus listener).
    post_frame_handle: Option<flui_scheduler::PostFrameHandle>,
    /// The current IME attach's cursor-area loop alive-flag, if a loop is
    /// currently running. `None` when no loop is running (never attached,
    /// or already blurred/disposed).
    ///
    /// A *fresh* `Rc<Cell<bool>>` is minted per attach (ADR-0032): sharing
    /// one flag across attaches would let a stale queued firing from a
    /// PREVIOUS attach flip it back to `true` behavior on a blur→refocus,
    /// resurrecting a loop that should have died, or running two loops at
    /// once. Detach/dispose flips THIS slot's flag `false` and takes it out
    /// of the slot; a loop closure that already holds its own clone still
    /// sees the flip (shared `Cell`) and dies on its next firing.
    cursor_area_alive: Rc<RefCell<Option<Rc<Cell<bool>>>>>,
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
            inner_anchor: flui_objects::SubtreeAnchor::new(),
            parent: None,
            controller: self.controller.clone(),
            controller_listener_id: None,
            rebuild_notifier: flui_foundation::notifier::ChangeNotifier::new(),
            focus_listener_id: None,
            ime_focus_listener_id: None,
            ime_handle: None,
            ime_token: Rc::new(RefCell::new(None)),
            post_frame_handle: None,
            cursor_area_alive: Rc::new(RefCell::new(None)),
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

        // 5. Attach/detach the IME client on this field's own focus
        //    transitions. `text_input_handle()` is a frame capability —
        //    acquired here, in `init_state`, never in `build` (see
        //    `BuildContext::text_input_handle`'s doc) — and stored so the
        //    focus-listener closure below (which cannot borrow `&mut self`)
        //    and `dispose` can both reach it. `post_frame_handle()` and
        //    `pipeline_owner()` are acquired alongside it for the same
        //    reason — the IME cursor-area loop (ADR-0032) they drive is
        //    started/stopped by that same closure.
        self.ime_handle = ctx.text_input_handle();
        self.post_frame_handle = ctx.post_frame_handle();
        let ime_handle_for_focus = self.ime_handle.clone();
        let post_frame_handle_for_focus = self.post_frame_handle.clone();
        let pipeline_owner_for_focus = ctx.pipeline_owner();
        let inner_anchor_for_focus = self.inner_anchor.clone();
        let controller_for_ime = self.controller.clone();
        let ime_token_for_focus = Rc::clone(&self.ime_token);
        let cursor_area_alive_for_focus = Rc::clone(&self.cursor_area_alive);
        self.ime_focus_listener_id = Some(FocusManager::global().add_listener(Rc::new(
            move |previous, current| {
                let was_focused = previous == Some(node_id);
                let now_focused = current == Some(node_id);
                if was_focused == now_focused {
                    return;
                }
                let Some(handle) = &ime_handle_for_focus else {
                    return;
                };
                if now_focused {
                    // Fresh per-attach state (ADR-0032): `last_sent` resets
                    // so a brand-new IME session always gets its first rect
                    // even at an unchanged caret position, and `alive` is a
                    // NEW flag so a stale queued firing from a previous
                    // attach (see the field's `cursor_area_alive` doc) can
                    // never resurrect this session or run alongside it.
                    let last_sent: Rc<Cell<Option<Bounds<Pixels>>>> = Rc::new(Cell::new(None));
                    let alive = Rc::new(Cell::new(true));
                    *cursor_area_alive_for_focus.borrow_mut() = Some(Rc::clone(&alive));

                    let controller_for_callback = controller_for_ime.clone();
                    let last_sent_for_ime_event = Rc::clone(&last_sent);
                    let token = handle.attach(Rc::new(move |event: &ImeEvent| {
                        apply_ime_event(&controller_for_callback, event);
                        // The backend may have restarted the IME session
                        // (`Enabled` re-fires on that restart) — clearing
                        // `last_sent` guarantees the new session gets a
                        // fresh rect instead of the dedupe cache silently
                        // suppressing it.
                        if matches!(event, ImeEvent::Enabled) {
                            last_sent_for_ime_event.set(None);
                        }
                    }));
                    *ime_token_for_focus.borrow_mut() = token;

                    if let Some(post_frame) = post_frame_handle_for_focus.clone() {
                        CursorAreaLoop {
                            post_frame,
                            pipeline_owner: pipeline_owner_for_focus.clone(),
                            inner_anchor: inner_anchor_for_focus.clone(),
                            text_input: handle.clone(),
                            alive,
                            last_sent,
                        }
                        .schedule();
                    } else {
                        tracing::warn!(
                            "IME cursor-area tracking not started: no post-frame handle \
                             installed (the platform candidate window will not follow \
                             the caret)"
                        );
                    }
                } else {
                    if let Some(token) = ime_token_for_focus.borrow_mut().take() {
                        handle.detach(token);
                    }
                    if let Some(alive) = cursor_area_alive_for_focus.borrow_mut().take() {
                        alive.set(false);
                    }
                }
            },
        )));
    }

    fn did_update_view(&mut self, _old_view: &EditableText, new_view: &EditableText) {
        // A field disabled while focused must not keep the caret and keyboard
        // input — mirrors Flutter's `TextField`/`EditableText` unfocusing when
        // `enabled` flips false mid-focus. `FocusNode::set_can_request_focus`
        // itself releases primary focus on a true-to-false change (Flutter's
        // `FocusNode.canRequestFocus` setter semantics), so this call alone
        // covers the unfocus — no separate `has_primary_focus` check needed.
        //
        // Load-bearing order: this runs BEFORE `set_focus_node_id(None)`
        // below. `FocusManager::unfocus` notifies every registered listener
        // with the (previous, current) pair; an enclosing decorated field
        // (`flui_material::TextField`) compares that pair against
        // `controller.focus_node_id()` to detect ITS OWN focus-loss
        // transition. Clearing the id first would make that comparison
        // vacuous by the time the notification fires (the id is already
        // gone), silently masking the transition from any such listener.
        self.focus_node.set_can_request_focus(new_view.enabled);
        if new_view.enabled {
            self.controller
                .set_focus_node_id(Some(self.focus_node.id()));
        } else {
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
        let inner_anchor = self.inner_anchor.clone();

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
                    inner_anchor.clone(),
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
        // Remove the IME focus-change listener we registered in init_state.
        if let Some(id) = self.ime_focus_listener_id.take() {
            FocusManager::global().remove_listener(id);
        }

        // Detach the IME client if this field still has one attached — the
        // ADR-0030 detach-on-dispose contract. A field unmounted while
        // focused is not guaranteed a focus-loss notification from
        // `detach_node` below (it clears primary focus without promising a
        // listener fires), so this is the one path that unconditionally
        // closes the IME session on unmount. Harmless no-op if the field
        // already blurred (and so already detached) before unmounting.
        if let Some(token) = self.ime_token.borrow_mut().take()
            && let Some(handle) = &self.ime_handle
        {
            handle.detach(token);
        }

        // Stop the IME cursor-area loop (ADR-0032) if one is running — the
        // same unconditional-on-unmount contract as the IME token detach
        // just above, and independent of it: a field unmounted while
        // focused is not guaranteed a blur notification, so this is the one
        // path that always flips the current attach's alive flag false,
        // whether or not the field ever blurred first.
        if let Some(alive) = self.cursor_area_alive.borrow_mut().take() {
            alive.set(false);
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

        // Deliberately NOT disposed here: `self.rebuild_notifier` is also
        // held by the `AnimatedBuilder` this state's own `build()` output
        // wraps around (`Arc::new(self.rebuild_notifier.clone())`), and that
        // child element's own `on_unmount` calls `remove_listener` on its
        // clone as part of the SAME unmount sweep. `ViewState::dispose`
        // (this method) runs before that child unmounts.
        //
        // `ChangeNotifier::remove_listener` is a safe no-op after dispose
        // (Flutter parity — see `Listenable::remove_listener`'s doc), so
        // calling `dispose()` here first would no longer panic against the
        // child's later `remove_listener` call. It is still left undone: an
        // explicit `dispose()` would mark the shared notifier disposed while
        // the child's subscription is still live, which is unnecessary
        // churn for no behavioral gain here — letting the notifier's `Arc`
        // refcount reach zero naturally (once every clone — this state's
        // and the child element's — is gone) is sufficient, since nothing
        // reads its disposed-flag on this path.
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Route one delivered [`ImeEvent`] to the matching
/// [`TextEditingController`] composing operation — the IME client callback
/// installed in `EditableTextState::init_state`.
///
/// `Enabled` is purely informational (nothing on the controller models
/// "composition is available but not yet started"); the other three variants
/// map 1:1 onto the controller's composing methods, which carry the
/// replace-vs-insert and clamping rules — see each method's doc.
fn apply_ime_event(controller: &TextEditingController, event: &ImeEvent) {
    match event {
        ImeEvent::Preedit { text, cursor } => controller.set_composing_text(text, *cursor),
        ImeEvent::Commit(text) => controller.commit_text(text),
        ImeEvent::Disabled => controller.clear_composing(),
        // Covers `Enabled` (purely informational, see this fn's doc) and any
        // future variant a winit upgrade adds — `ImeEvent` is
        // `#[non_exhaustive]`, so an unhandled new variant is a no-op here
        // until this match is revisited, never a broken build for an
        // unrelated crate bump.
        _ => {}
    }
}

/// The self-rescheduling IME cursor-area tracking loop (ADR-0032).
///
/// One instance is created per IME attach (focus gain). Each firing reads the
/// caret's current global rect and forwards it through
/// [`TextInputHandle::set_cursor_area`] when it changed, then reschedules
/// itself for the next completed frame — Flutter's own
/// `_schedulePeriodicPostFrameCallbacks` cadence (`editable_text.dart`, tag
/// `3.44.0`), dormant whenever no frame runs. `Clone` because
/// [`flui_scheduler::PostFrameHandle::schedule_local`] takes an `FnOnce`, so the only way to
/// make it self-rescheduling without boxing a trait object is for each
/// firing to consume `self` and, if still alive, construct the next firing's
/// closure from a fresh clone of the same capture.
#[derive(Clone)]
struct CursorAreaLoop {
    post_frame: flui_scheduler::PostFrameHandle,
    pipeline_owner: Option<Arc<RwLock<PipelineOwner>>>,
    /// The `EditableTextRenderView`'s own inner anchor (ADR-0032) — see
    /// `EditableTextState::inner_anchor`'s doc.
    inner_anchor: flui_objects::SubtreeAnchor,
    text_input: TextInputHandle,
    /// Per-attach liveness flag — see `EditableTextState::cursor_area_alive`'s
    /// doc for why it is fresh per attach. Checked at the START of every
    /// firing so a callback already queued when the attach ended dies
    /// silently instead of resurrecting a stale loop or running alongside a
    /// newer one.
    alive: Rc<Cell<bool>>,
    /// The last rect actually sent. Fresh per attach (never shared across
    /// attaches, see `EditableTextState::init_state`'s IME focus listener) —
    /// a brand-new IME session must always get the first rect, even at an
    /// unchanged caret position.
    last_sent: Rc<Cell<Option<Bounds<Pixels>>>>,
}

#[cfg(test)]
thread_local! {
    /// Test-only probe: counts every successful `PostFrameHandle::
    /// schedule_local` registration `CursorAreaLoop::schedule` makes.
    ///
    /// This exists because "no new `cursor_area_calls`" is NOT sufficient
    /// evidence that the loop stopped: once `inner_anchor` clears (on
    /// unmount), `global_caret_rect` returns `None` regardless of whether
    /// the loop is still alive, so a zombie loop that keeps rescheduling
    /// itself forever looks send-silent and indistinguishable from a
    /// correctly-stopped one under a sends-only assertion. Counting actual
    /// reschedule registrations is what tells them apart — see
    /// `loop_stops_rescheduling_after_dispose_while_still_focused`.
    static CURSOR_AREA_RESCHEDULE_COUNT: Cell<usize> = const { Cell::new(0) };
}

/// The number of `CursorAreaLoop::schedule` registrations made so far on
/// this thread. Test-only; see `CURSOR_AREA_RESCHEDULE_COUNT`'s doc.
#[cfg(test)]
fn cursor_area_reschedule_count() -> usize {
    CURSOR_AREA_RESCHEDULE_COUNT.with(Cell::get)
}

impl CursorAreaLoop {
    /// Register the next firing. Every `schedule_local` failure is warned,
    /// never silent: a loop that stops rescheduling without a diagnostic is
    /// a candidate window stuck at (0, 0) with no signal anything is wrong.
    fn schedule(self) {
        let post_frame = self.post_frame.clone();
        match post_frame.schedule_local(move |_timing| self.fire()) {
            Ok(()) => {
                #[cfg(test)]
                CURSOR_AREA_RESCHEDULE_COUNT.with(|count| count.set(count.get() + 1));
            }
            Err(error) => {
                tracing::warn!(
                    ?error,
                    "IME cursor-area tick could not be (re)scheduled; the platform \
                     candidate window will stop following the caret"
                );
            }
        }
    }

    fn fire(self) {
        if !self.alive.get() {
            return;
        }
        // A `None` read is a transient miss (the anchored subtree unmounted
        // mid-rebuild, or a transform is momentarily unavailable) — skip
        // this firing's send but keep the loop alive. Only `alive == false`
        // ever stops rescheduling.
        if let Some(rect) = self.global_caret_rect()
            && Some(rect) != self.last_sent.get()
        {
            self.text_input.set_cursor_area(rect);
            self.last_sent.set(Some(rect));
        }
        self.schedule();
    }

    /// The IME candidate window's current target rect in window-root-space
    /// logical pixels: `inner_anchor`'s committed transform to the render
    /// root, applied to the anchored `RenderEditable` child's composing
    /// region rect when one is active, falling back to its collapsed caret
    /// rect otherwise — Flutter's own `_updateComposingRectIfNeeded` order
    /// (`editable_text.dart`, tag `3.44.0`: prefer the composing rect,
    /// fall back to the caret rect when none is available). ADR-0033
    /// upgrades this loop from the caret-rect-only reduction ADR-0032
    /// originally landed.
    fn global_caret_rect(&self) -> Option<Bounds<Pixels>> {
        let anchor_id = self.inner_anchor.get()?;
        let owner = self.pipeline_owner.as_ref()?.read();
        let root_id = owner.root_id()?;
        let tree = owner.render_tree();
        let editable_id = *tree.children(anchor_id).first()?;
        let editable = tree
            .get(editable_id)?
            .as_box()?
            .render_object()
            .downcast_ref::<RenderEditable>()?; // PORT-CHECK-OK-DOWNCAST: ADR-0032 IME cursor-area loop reaches the one concrete render object type it knows sits under `inner_anchor` (an `EditableTextRenderView`'s `RenderEditable`) through the storage layer's `&dyn RenderObject<BoxProtocol>` erasure — see docs/PORT.md FR-033/widgets.
        let local_rect = editable
            .rect_for_composing_range()
            .unwrap_or_else(|| editable.caret_local_rect());
        let transform = owner.transform_to(anchor_id, root_id)?;
        Some(bounds_from_rect(transform.transform_rect(&local_rect)))
    }
}

/// `Rect` (min/max corners) to `Bounds` (origin/size) — `PlatformTextInput::
/// set_ime_cursor_area`'s parameter convention, matching `PlatformWindow::
/// bounds`.
fn bounds_from_rect(rect: Rect) -> Bounds<Pixels> {
    Bounds::new(
        Point::new(rect.min.x, rect.min.y),
        flui_types::Size::new(rect.width(), rect.height()),
    )
}

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
                // Suppression contract (`ImeEvent`'s doc): suppress
                // `Key::Character` insertion ONLY while a composition is
                // active. Winit withholds `KeyboardInput` during
                // composition and immediately after a commit, so this path
                // exists mainly for backends/tests that dispatch a
                // character key mid-preedit anyway — without the guard it
                // would double-insert alongside the IME's own commit.
                // Consumed either way: this field owns the key while
                // focused, composing or not.
                if !controller.is_composing() {
                    controller.insert_str(character_string.as_str());
                }
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
    /// The IME composing region to underline, gated on `enabled &&
    /// has_primary_focus()` by [`build_field_view`] — the FLUI analog of
    /// Flutter's `buildTextSpan`'s `withComposing: !widget.readOnly` (plus
    /// its own focus gating), named rather than a direct port since no
    /// `readOnly` field exists (see [`EditableText::enabled`]'s doc).
    composing_range: Option<Range<usize>>,
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
            .with_composing_range(self.composing_range.clone())
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
///
/// Wraps `EditableTextRenderView` directly in `inner_anchor` (the inner
/// anchor, ADR-0032): a second, inner `SubtreeAnchor` whose only job is to publish
/// exactly the editable's own `RenderId`, so the IME cursor-area loop's
/// `transform_to` starts right at the editable — not at the outer `anchor`
/// wrapping this whole field (which also spans the `AnimatedBuilder` between
/// the two, zero-offset by convention only).
fn build_field_view(
    controller: &TextEditingController,
    focus_node: &Arc<FocusNode>,
    caret_height: f32,
    caret_color: Color,
    enabled: bool,
    text_style: Option<TextStyle>,
    inner_anchor: flui_objects::SubtreeAnchor,
) -> BoxedView {
    // `enabled` is defensive here: `did_update_view` already unfocuses a
    // field that becomes disabled while focused, so `has_primary_focus`
    // should already be `false` by the time this runs.
    let focused = enabled && focus_node.has_primary_focus();
    crate::navigator::AnchoredBox::new(
        inner_anchor,
        EditableTextRenderView {
            text: controller.text(),
            caret_byte_offset: controller.caret_byte_offset(),
            show_caret: focused && !controller.caret_hidden_by_ime(),
            // Composing-region underline gated on the same `focused` check
            // as `show_caret` — Flutter's `buildTextSpan`'s
            // `withComposing: !widget.readOnly` plus its focus gating
            // (`_EditableTextState.buildTextSpan`, `editable_text.dart`,
            // tag `3.44.0`): an unfocused field must not keep painting a
            // stale composing underline for text it no longer owns input
            // for.
            composing_range: if focused {
                controller.composing_range()
            } else {
                None
            },
            caret_height,
            caret_color,
            text_style,
        },
    )
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
    /// node — `did_update_view`'s `set_can_request_focus(false)` call (see
    /// [`EditableText::enabled`]'s doc comment) releases primary focus itself,
    /// matching Flutter's `FocusNode.canRequestFocus` setter.
    ///
    /// Red-check: pass `true` instead of `new_view.enabled` to
    /// `set_can_request_focus` in `did_update_view` — the node stays
    /// primary-focused and the first assertion fails.
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
            composing_range: None,
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
            composing_range: None,
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

    // ------------------------------------------------------------------
    // IME integration
    //
    // `mount_with_ime` installs a `TextInputHandle` wired to
    // `flui_interaction::TextInputRegistry::global()` directly (no
    // `flui-app`/`PlatformWindow` involved — that half of the ADR-0030
    // bridge, `set_ime_allowed`, is covered by `flui-app`'s own
    // `ime_binding_bridge` tests plus a dedicated end-to-end test wired
    // through a real binding). These tests drive the SAME registry the
    // field attaches to, so `registry.dispatch(...)` reaches the mounted
    // field exactly the way `AppBinding::handle_input`'s `PlatformInput::Ime`
    // arm does in production.
    // ------------------------------------------------------------------

    fn dispatch_ime(event: &flui_types::ImeEvent) {
        flui_interaction::TextInputRegistry::global().dispatch(event);
    }

    fn character_key_event(ch: char) -> flui_interaction::events::KeyEvent {
        use flui_interaction::events::Code;
        use flui_interaction::testing::input::KeyEventBuilder;
        KeyEventBuilder::new(Code::KeyA)
            .with_key(Key::Character(ch.to_string()))
            .with_state(KeyState::Down)
            .build()
    }

    /// A root that can drop its `EditableText`, so a still-focused field can
    /// be unmounted out from under its own focus — the dispose-contract
    /// scenario. Mirrors `hero_controller_tests::Root`'s show/hide shape.
    #[derive(Clone)]
    struct ImeUnmountRoot {
        controller: TextEditingController,
        show: bool,
    }

    impl View for ImeUnmountRoot {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateless(self)
        }
    }

    impl StatelessView for ImeUnmountRoot {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            if self.show {
                EditableText::new(self.controller.clone()).boxed()
            } else {
                crate::Text::new("gone").boxed()
            }
        }
    }

    /// Red-check: skip the `TextInputHandle::attach` call on focus gain (make
    /// the IME focus listener a no-op) — this test's `active_count`
    /// assertion fails and the later dispatch reaches nobody.
    #[test]
    fn focus_gain_attaches_an_ime_client_and_routes_preedit_to_the_controller() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let _harness = crate::test_harness::mount_with_ime(EditableText::new(controller.clone()));
        let node_id = controller
            .focus_node_id()
            .expect("an enabled field publishes its node");

        FocusManager::global().request_focus(node_id);
        assert_eq!(
            flui_interaction::TextInputRegistry::global().active_count(),
            1,
            "focus gain must attach an IME client"
        );

        dispatch_ime(&flui_types::ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: Some((0, 2)),
        });

        assert_eq!(controller.text(), "ni");
        assert_eq!(controller.composing_range(), Some(0..2));
    }

    #[test]
    fn commit_replaces_the_composing_region_through_the_attached_client() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let _harness = crate::test_harness::mount_with_ime(EditableText::new(controller.clone()));
        let node_id = controller.focus_node_id().expect("published node");
        FocusManager::global().request_focus(node_id);

        dispatch_ime(&flui_types::ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: Some((2, 2)),
        });
        dispatch_ime(&flui_types::ImeEvent::Commit("你".to_string()));

        assert_eq!(controller.text(), "你");
        assert!(
            !controller.is_composing(),
            "a commit must clear the composing region"
        );
    }

    /// Red-check: remove the `if !controller.is_composing()` guard in
    /// `build_key_handler`'s `Key::Character` arm — the dispatched key
    /// inserts "n" on top of the preedit's own "n" and this test's text
    /// assertion fails (`"nn"` instead of `"n"`).
    #[test]
    fn character_key_during_active_composition_does_not_double_insert() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let _harness = crate::test_harness::mount_with_ime(EditableText::new(controller.clone()));
        let node_id = controller.focus_node_id().expect("published node");
        FocusManager::global().request_focus(node_id);

        dispatch_ime(&flui_types::ImeEvent::Preedit {
            text: "n".to_string(),
            cursor: Some((1, 1)),
        });
        assert_eq!(controller.text(), "n");

        let handled = FocusManager::global().dispatch_key_event(&character_key_event('n'));
        assert!(handled, "the focused field must still consume the key");
        assert_eq!(
            controller.text(),
            "n",
            "a Key::Character delivered during active composition must not \
             insert on top of the preedit"
        );
    }

    /// The plain-typing case the suppression guard must not break: IME is
    /// attached (the field is focused, `TextInputRegistry` has a client) but
    /// no preedit is active, so ordinary characters insert exactly as they
    /// would with no IME composition involved at all.
    #[test]
    fn character_key_with_ime_attached_but_no_active_preedit_inserts_normally() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let _harness = crate::test_harness::mount_with_ime(EditableText::new(controller.clone()));
        let node_id = controller.focus_node_id().expect("published node");
        FocusManager::global().request_focus(node_id);
        assert!(!controller.is_composing(), "precondition: no preedit yet");

        let handled = FocusManager::global().dispatch_key_event(&character_key_event('x'));
        assert!(handled);
        assert_eq!(controller.text(), "x");
    }

    /// Red-check: drop the `else if let Some(token) = ... handle.detach`
    /// branch in the IME focus listener — this test's `active_count`
    /// assertion after `unfocus()` fails (stays 1).
    #[test]
    fn blur_detaches_the_ime_client() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let _harness = crate::test_harness::mount_with_ime(EditableText::new(controller.clone()));
        let node_id = controller.focus_node_id().expect("published node");
        FocusManager::global().request_focus(node_id);
        assert_eq!(
            flui_interaction::TextInputRegistry::global().active_count(),
            1
        );

        FocusManager::global().unfocus();
        assert_eq!(
            flui_interaction::TextInputRegistry::global().active_count(),
            0,
            "blur must detach the IME client"
        );
    }

    /// The ADR-0030 detach-on-dispose contract: a field unmounted while
    /// still focused must not leave a stale IME client attached, even though
    /// unmounting never delivers a `previous == Some(node_id)` focus
    /// transition to the field's own listener.
    ///
    /// Red-check: remove the explicit `handle.detach(token)` call from
    /// `EditableTextState::dispose` — this test's final `active_count`
    /// assertion fails (leaks the attached client).
    #[test]
    fn unmount_while_focused_detaches_the_ime_client() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let mut harness = crate::test_harness::mount_with_ime(ImeUnmountRoot {
            controller: controller.clone(),
            show: true,
        });
        let node_id = controller.focus_node_id().expect("published node");
        FocusManager::global().request_focus(node_id);
        assert_eq!(
            flui_interaction::TextInputRegistry::global().active_count(),
            1
        );

        harness.swap_root(ImeUnmountRoot {
            controller: controller.clone(),
            show: false,
        });

        assert_eq!(
            flui_interaction::TextInputRegistry::global().active_count(),
            0,
            "unmounting a still-focused field must detach its IME client \
             (the ADR-0030 dispose contract)"
        );
    }

    /// Red-check: drop the `guard.text.replace_range(range, "")` in
    /// `TextEditingController::clear_composing` (keep only the marker
    /// clear) — this test's text assertion fails, keeping the uncommitted
    /// preedit instead of stripping it.
    #[test]
    fn disabled_mid_preedit_strips_the_composing_slice_through_the_attached_client() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::with_text("Hello ");
        let _harness = crate::test_harness::mount_with_ime(EditableText::new(controller.clone()));
        let node_id = controller.focus_node_id().expect("published node");
        FocusManager::global().request_focus(node_id);

        dispatch_ime(&flui_types::ImeEvent::Preedit {
            text: "wor".to_string(),
            cursor: Some((3, 3)),
        });
        assert_eq!(controller.text(), "Hello wor");

        dispatch_ime(&flui_types::ImeEvent::Disabled);

        assert_eq!(
            controller.text(),
            "Hello ",
            "a mid-composition Disabled must strip the composing slice \
             (winit semantics, a documented divergence from Flutter's \
             TextInputConnection.connectionClosed)"
        );
        assert!(!controller.is_composing());
    }

    // ------------------------------------------------------------------
    // IME cursor-area tracking (ADR-0032)
    //
    // Focusing/blurring in these tests runs inside `harness.
    // enter_owner_scope(...)`, unlike the IME-composition tests above:
    // the cursor-area loop's `PostFrameHandle::schedule_local` call only
    // succeeds while the harness's local post-frame lane is the active
    // top of the lane stack — exactly the shape `HeadlessBinding::
    // pump_frame` and production's `realm.enter` share (see
    // `CursorAreaLoop`'s doc). A focus change dispatched with no active
    // lane still attaches/detaches the IME client correctly (that part
    // needs no lane), it just never starts the loop — which is why the
    // composition tests above, which never call `enter_owner_scope`,
    // still pass unaffected by this feature.
    //
    // Transient-`None` resilience (a fully in-place red-check for "skip
    // the send, keep the loop alive" — one of `CursorAreaLoop::fire`'s
    // two branches) is not constructed here: forcing `global_caret_rect`
    // to observe the inner anchor mid-unmount deterministically would
    // require reaching into the pipeline mid-rebuild, which this
    // harness has no cheap hook for. The branch itself is exercised
    // structurally by every test below during the ordinary frame in which
    // the tree is *not* yet built (`mount_with_ime`'s own initial
    // attach), and its shape (`if let Some(rect) = ... { send } ;
    // self.schedule()` — the reschedule is unconditional, not gated on
    // the `Some` arm) is the same one line the `loop_stops_sending_after_*`
    // tests below would fail to distinguish from a real stop if it were
    // wrong.
    // ------------------------------------------------------------------

    /// Focusing a field under a translated ancestor (`Padding`) sends the
    /// caret's rect through `TextInputHandle::set_cursor_area` exactly once,
    /// with the ancestor's offset folded in — proving both the basic
    /// send-on-focus contract and that `transform_to` (not the local rect
    /// alone) is what reaches the platform.
    ///
    /// Red-check: skip `transform.transform_rect(&local_rect)` in
    /// `CursorAreaLoop::global_caret_rect` (send the untransformed local
    /// rect) — the origin assertions below fail (they'd read `(0, 0)`
    /// instead of the padding offset).
    #[test]
    fn focusing_sends_the_exact_caret_rect_including_ancestor_padding() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let mut harness = crate::test_harness::mount_with_ime(
            crate::Padding::only(20.0, 10.0, 0.0, 0.0).child(EditableText::new(controller.clone())),
        );
        let node_id = controller.focus_node_id().expect("published node");

        harness.enter_owner_scope(|| {
            FocusManager::global().request_focus(node_id);
        });
        assert!(
            harness.cursor_area_calls().is_empty(),
            "focus gain schedules the first tick but must not send synchronously"
        );

        harness.tick();

        let calls = harness.cursor_area_calls();
        assert_eq!(
            calls.len(),
            1,
            "exactly one send on the frame after focus gain"
        );
        assert_eq!(
            calls[0].origin,
            flui_types::Point::new(
                flui_types::geometry::px(20.0),
                flui_types::geometry::px(10.0)
            ),
            "the sent rect must include the Padding ancestor's offset, not just \
             the caret's local position: {:?}",
            calls[0]
        );
        assert_eq!(
            calls[0].size,
            flui_types::Size::new(
                flui_types::geometry::px(2.0),
                flui_types::geometry::px(18.0)
            ),
            "the sent rect must carry the caret's own width/height: {:?}",
            calls[0]
        );
    }

    /// Committing a character moves the caret, and the next pump sends a new
    /// rect with the x coordinate advanced.
    #[test]
    fn caret_advance_sends_a_new_rect_with_x_advanced_after_a_commit() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let mut harness =
            crate::test_harness::mount_with_ime(EditableText::new(controller.clone()));
        let node_id = controller.focus_node_id().expect("published node");

        harness.enter_owner_scope(|| {
            FocusManager::global().request_focus(node_id);
        });
        harness.tick();
        let first = *harness
            .cursor_area_calls()
            .first()
            .expect("one send after focus gain");

        controller.insert_str("m");
        harness.tick();

        let calls = harness.cursor_area_calls();
        assert_eq!(
            calls.len(),
            2,
            "the caret moving after a commit must trigger exactly one more send"
        );
        assert!(
            calls[1].origin.x.get() > first.origin.x.get(),
            "the caret's x must advance after inserting a character: {:?} -> {:?}",
            first,
            calls[1]
        );
    }

    /// Two unchanged frames send exactly one call (dedupe), but a
    /// blur→refocus at the SAME caret position sends again — the
    /// attach-reset half that keeps dedupe from suppressing a brand-new IME
    /// session forever.
    ///
    /// Red-check (dedupe half): drop the `Some(rect) != self.last_sent.get()`
    /// guard in `CursorAreaLoop::fire` — the unchanged-frame assertion below
    /// fails (every tick resends).
    ///
    /// Red-check (attach-reset half): make `last_sent` a field shared across
    /// attaches instead of a fresh `Rc::new(Cell::new(None))` per attach in
    /// `init_state`'s IME focus listener — the post-refocus assertion fails
    /// (the old cache suppresses the resend).
    #[test]
    fn dedupes_unchanged_frames_and_resends_after_a_refocus_at_the_same_position() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let mut harness =
            crate::test_harness::mount_with_ime(EditableText::new(controller.clone()));
        let node_id = controller.focus_node_id().expect("published node");

        harness.enter_owner_scope(|| {
            FocusManager::global().request_focus(node_id);
        });
        harness.tick();
        assert_eq!(harness.cursor_area_calls().len(), 1);

        harness.tick();
        harness.tick();
        assert_eq!(
            harness.cursor_area_calls().len(),
            1,
            "two further unchanged frames must not resend"
        );

        harness.enter_owner_scope(|| {
            FocusManager::global().unfocus();
        });
        harness.tick();
        harness.enter_owner_scope(|| {
            FocusManager::global().request_focus(node_id);
        });
        harness.tick();

        assert_eq!(
            harness.cursor_area_calls().len(),
            2,
            "refocusing at an unchanged caret position must resend — a new IME \
             session always gets its first rect"
        );
    }

    /// `ImeEvent::Enabled` clears the current attach's dedupe cache — the
    /// backend may restart the IME session without any focus change, and
    /// that restart must not be silently absorbed by `last_sent`: the
    /// resumed session needs its own first send even at an unchanged caret
    /// position.
    ///
    /// Red-check: drop the `last_sent_for_ime_event.set(None)` call in the
    /// IME event callback's `ImeEvent::Enabled` arm (`init_state`) — the
    /// final assertion fails (dedupe suppresses the resend).
    #[test]
    fn ime_enabled_event_clears_the_dedupe_cache_and_forces_a_resend() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let mut harness =
            crate::test_harness::mount_with_ime(EditableText::new(controller.clone()));
        let node_id = controller.focus_node_id().expect("published node");

        harness.enter_owner_scope(|| {
            FocusManager::global().request_focus(node_id);
        });
        harness.tick();
        assert_eq!(harness.cursor_area_calls().len(), 1);

        // An unchanged frame first, to prove the dedupe cache is actually
        // populated (not merely empty from a fresh attach) before `Enabled`
        // clears it.
        harness.tick();
        assert_eq!(
            harness.cursor_area_calls().len(),
            1,
            "precondition: an unchanged frame must dedupe before Enabled fires"
        );

        dispatch_ime(&flui_types::ImeEvent::Enabled);
        harness.tick();

        assert_eq!(
            harness.cursor_area_calls().len(),
            2,
            "ImeEvent::Enabled must clear the dedupe cache so an unchanged \
             caret position resends on the next frame"
        );
    }

    /// Blurring stops the loop: no further sends, even once the controller
    /// keeps changing after the blur.
    ///
    /// Red-check: drop the `alive.set(false)` call in the IME focus
    /// listener's blur branch (`init_state`) — this test's final assertion
    /// fails (the loop keeps sending after blur).
    #[test]
    fn loop_stops_sending_after_blur() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let mut harness =
            crate::test_harness::mount_with_ime(EditableText::new(controller.clone()));
        let node_id = controller.focus_node_id().expect("published node");

        harness.enter_owner_scope(|| {
            FocusManager::global().request_focus(node_id);
        });
        harness.tick();
        assert_eq!(harness.cursor_area_calls().len(), 1);

        harness.enter_owner_scope(|| {
            FocusManager::global().unfocus();
        });
        harness.tick();
        let calls_after_blur = harness.cursor_area_calls().len();

        controller.insert_str("z");
        harness.tick();
        harness.tick();

        assert_eq!(
            harness.cursor_area_calls().len(),
            calls_after_blur,
            "a blurred field's loop must not send again even after the caret \
             moves and further frames pump"
        );
    }

    /// Unmounting a still-focused field stops the loop's RESCHEDULING, not
    /// merely its sends — the ADR-0030 detach-on-dispose contract extended
    /// to the cursor-area loop, and no panic either.
    ///
    /// "No new `cursor_area_calls`" alone is NOT sufficient evidence here:
    /// `RenderSubtreeAnchor::detach` clears `inner_anchor` on unmount, so
    /// `global_caret_rect` returns `None` regardless of whether the loop is
    /// still alive — a zombie loop that kept rescheduling itself forever
    /// (never sending, but never stopping either — a permanent
    /// once-per-frame `schedule_local` registration leak) would look
    /// send-silent and pass a sends-only assertion. This test instead counts
    /// actual reschedule registrations via `cursor_area_reschedule_count`.
    ///
    /// Red-check: drop the `alive.set(false)` call in `EditableTextState::
    /// dispose` — the reschedule count keeps climbing on every subsequent
    /// `tick()` instead of holding steady once the dispose frame settles.
    #[test]
    fn loop_stops_rescheduling_after_dispose_while_still_focused() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let mut harness = crate::test_harness::mount_with_ime(ImeUnmountRoot {
            controller: controller.clone(),
            show: true,
        });
        let node_id = controller.focus_node_id().expect("published node");

        harness.enter_owner_scope(|| {
            FocusManager::global().request_focus(node_id);
        });
        harness.tick();
        assert_eq!(harness.cursor_area_calls().len(), 1);

        // `swap_root` disposes the old (still-focused) field within its own
        // pumped frame: dispose runs during that frame's build phase,
        // before the SAME frame's post-frame phase drains the `fire()`
        // queued by the `tick()` above — so a correctly stopped loop must
        // not reschedule even once more here.
        harness.swap_root(ImeUnmountRoot {
            controller: controller.clone(),
            show: false,
        });
        let calls_after_unmount = harness.cursor_area_calls().len();
        let reschedules_after_dispose_frame = cursor_area_reschedule_count();

        // Further frames must not resurrect scheduling, and must not panic.
        controller.insert_str("y");
        harness.tick();
        harness.tick();

        assert_eq!(
            harness.cursor_area_calls().len(),
            calls_after_unmount,
            "unmounting a still-focused field must not send further cursor \
             areas"
        );
        assert_eq!(
            cursor_area_reschedule_count(),
            reschedules_after_dispose_frame,
            "a disposed-while-focused field must not keep rescheduling its \
             cursor-area loop — a zombie loop would reschedule once per \
             frame forever even though its sends look silent"
        );
    }

    /// A blur immediately followed by a refocus, both inside the SAME
    /// active-lane scope (no intervening pump) — the scenario a shared
    /// alive-flag across attaches would double-loop: the stale queued
    /// firing from the blurred attach would resurrect (share `true` with
    /// the new attach) instead of dying, and the next frame would send
    /// twice instead of once.
    ///
    /// Red-check: mint `alive`/`cursor_area_alive` once per field instead of
    /// fresh per attach — this test's delta assertion becomes `2` instead
    /// of `1`.
    #[test]
    fn blur_then_refocus_within_the_same_scope_leaves_exactly_one_live_loop() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let mut harness =
            crate::test_harness::mount_with_ime(EditableText::new(controller.clone()));
        let node_id = controller.focus_node_id().expect("published node");

        harness.enter_owner_scope(|| {
            FocusManager::global().request_focus(node_id);
        });
        harness.tick();
        assert_eq!(harness.cursor_area_calls().len(), 1);

        harness.enter_owner_scope(|| {
            FocusManager::global().unfocus();
            FocusManager::global().request_focus(node_id);
        });

        let before = harness.cursor_area_calls().len();
        harness.tick();
        let after = harness.cursor_area_calls().len();

        assert_eq!(
            after - before,
            1,
            "blur->refocus in one scope must leave exactly one live loop; a \
             leaked stale loop would double this frame's send count"
        );
    }

    // ------------------------------------------------------------------
    // Composing-region underline + hidden caret (ADR-0033)
    // ------------------------------------------------------------------

    /// Runs `f` against the mounted field's single `RenderEditable`, found
    /// by downcasting the one render object this widget mounts.
    fn with_render_editable<T>(
        harness: &crate::test_harness::Harness,
        f: impl FnOnce(&RenderEditable) -> T,
    ) -> Option<T> {
        let owner = harness.pipeline_owner();
        let owner = owner.read();
        let tree = owner.render_tree();
        let mut f = Some(f);
        for (_, node) in tree.iter() {
            let editable = node
                .as_box()
                .and_then(|b| b.render_object().downcast_ref::<RenderEditable>()); // PORT-CHECK-OK-DOWNCAST: test-only reach to the one concrete render object type this widget mounts, through the storage layer's `&dyn RenderObject<BoxProtocol>` erasure — same sanctioned boundary as `CursorAreaLoop::global_caret_rect` above; see docs/PORT.md FR-033/widgets.
            if let Some(editable) = editable {
                return f.take().map(|f| f(editable));
            }
        }
        None
    }

    /// Whether the mounted field's caret is currently painted.
    fn show_caret_flag(harness: &crate::test_harness::Harness) -> bool {
        with_render_editable(harness, RenderEditable::show_caret).unwrap_or(false)
    }

    /// The mounted field's composing-region rect, if any — `None` covers
    /// both "no `RenderEditable` found" and "no composing range active".
    fn composing_rect(harness: &crate::test_harness::Harness) -> Option<Rect> {
        with_render_editable(harness, RenderEditable::rect_for_composing_range).flatten()
    }

    /// The mounted field's collapsed caret rect — always geometry, per
    /// [`RenderEditable::caret_local_rect`]'s visibility-independence
    /// contract.
    fn caret_rect(harness: &crate::test_harness::Harness) -> Rect {
        with_render_editable(harness, RenderEditable::caret_local_rect)
            .expect("a mounted EditableText always has a RenderEditable")
    }

    /// `Preedit { cursor: None }` while focused hides the caret and starts
    /// painting the composing underline — the FLUI expression of Flutter's
    /// `buildTextSpan`'s composing-underline three-way split plus its
    /// hidden-caret case, both now implemented (ADR-0033).
    ///
    /// Red-check: drop the `!controller.caret_hidden_by_ime()` term from
    /// `build_field_view`'s `show_caret` expression — this test's
    /// `show_caret_flag` assertion fails (stays `true`).
    #[test]
    fn preedit_cursor_none_while_focused_hides_the_caret_and_starts_the_underline() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let mut harness =
            crate::test_harness::mount_with_ime(EditableText::new(controller.clone()));
        let node_id = controller.focus_node_id().expect("published node");
        harness.enter_owner_scope(|| {
            FocusManager::global().request_focus(node_id);
        });
        harness.tick();
        assert!(
            show_caret_flag(&harness),
            "precondition: the caret paints while focused with no composition"
        );

        dispatch_ime(&flui_types::ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: None,
        });
        harness.tick();

        assert!(
            !show_caret_flag(&harness),
            "cursor: None must hide the caret while composing"
        );
        assert!(
            composing_rect(&harness).is_some(),
            "an active composing range must produce composing geometry"
        );
    }

    /// The contrast case: `cursor: Some` keeps the caret visible alongside
    /// the composing underline.
    #[test]
    fn preedit_cursor_some_while_focused_keeps_the_caret_visible() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let mut harness =
            crate::test_harness::mount_with_ime(EditableText::new(controller.clone()));
        let node_id = controller.focus_node_id().expect("published node");
        harness.enter_owner_scope(|| {
            FocusManager::global().request_focus(node_id);
        });
        harness.tick();

        dispatch_ime(&flui_types::ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: Some((2, 2)),
        });
        harness.tick();

        assert!(
            show_caret_flag(&harness),
            "cursor: Some must leave the caret visible"
        );
        assert!(composing_rect(&harness).is_some());
    }

    /// A commit ends composition: the underline disappears and the caret is
    /// restored.
    #[test]
    fn commit_removes_the_underline_and_restores_the_caret() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let mut harness =
            crate::test_harness::mount_with_ime(EditableText::new(controller.clone()));
        let node_id = controller.focus_node_id().expect("published node");
        harness.enter_owner_scope(|| {
            FocusManager::global().request_focus(node_id);
        });
        harness.tick();

        dispatch_ime(&flui_types::ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: None,
        });
        harness.tick();
        assert!(
            !show_caret_flag(&harness),
            "precondition: caret hidden while composing"
        );

        dispatch_ime(&flui_types::ImeEvent::Commit("你".to_string()));
        harness.tick();

        assert!(
            composing_rect(&harness).is_none(),
            "a commit must remove the composing underline"
        );
        assert!(show_caret_flag(&harness), "a commit must restore the caret");
    }

    /// `Disabled` mid-composition (winit's connection-closed signal) also
    /// ends composition: underline gone, caret restored.
    #[test]
    fn disabled_removes_the_underline_and_restores_the_caret() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::with_text("Hello ");
        let mut harness =
            crate::test_harness::mount_with_ime(EditableText::new(controller.clone()));
        let node_id = controller.focus_node_id().expect("published node");
        harness.enter_owner_scope(|| {
            FocusManager::global().request_focus(node_id);
        });
        harness.tick();

        dispatch_ime(&flui_types::ImeEvent::Preedit {
            text: "wor".to_string(),
            cursor: None,
        });
        harness.tick();
        assert!(!show_caret_flag(&harness));

        dispatch_ime(&flui_types::ImeEvent::Disabled);
        harness.tick();

        assert!(composing_rect(&harness).is_none());
        assert!(show_caret_flag(&harness));
    }

    /// `Preedit("")` — winit's composition-cancel signal — ends composition
    /// through the full attached-client path: underline gone, caret
    /// restored, and plain typing works immediately after.
    #[test]
    fn empty_preedit_cancels_the_composition_through_the_attached_client() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let mut harness =
            crate::test_harness::mount_with_ime(EditableText::new(controller.clone()));
        let node_id = controller.focus_node_id().expect("published node");
        harness.enter_owner_scope(|| {
            FocusManager::global().request_focus(node_id);
        });
        harness.tick();

        dispatch_ime(&flui_types::ImeEvent::Preedit {
            text: "nihao".to_string(),
            cursor: Some((5, 5)),
        });
        harness.tick();
        assert_eq!(controller.text(), "nihao");

        dispatch_ime(&flui_types::ImeEvent::Preedit {
            text: String::new(),
            cursor: None,
        });
        harness.tick();

        assert_eq!(controller.text(), "");
        assert!(!controller.is_composing());
        assert!(composing_rect(&harness).is_none());
        assert!(show_caret_flag(&harness));

        let handled = FocusManager::global().dispatch_key_event(&character_key_event('x'));
        assert!(handled);
        assert_eq!(
            controller.text(),
            "x",
            "plain typing must work immediately after the cancel"
        );
    }

    /// The gating contract: an unfocused field must not keep passing a
    /// still-active composing range to the render view, even though blur
    /// does not itself end the composition (only detaches the IME client —
    /// see `blur_detaches_the_ime_client`).
    ///
    /// Red-check: drop the `if focused { ... } else { None }` gate around
    /// `composing_range` in `build_field_view` (pass
    /// `controller.composing_range()` unconditionally) — the final
    /// assertion's inversion holds: `composing_rect` stays `Some` after
    /// blur instead of becoming `None`.
    #[test]
    fn unfocus_mid_composition_stops_passing_the_composing_range() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let mut harness =
            crate::test_harness::mount_with_ime(EditableText::new(controller.clone()));
        let node_id = controller.focus_node_id().expect("published node");
        harness.enter_owner_scope(|| {
            FocusManager::global().request_focus(node_id);
        });
        harness.tick();

        dispatch_ime(&flui_types::ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: Some((2, 2)),
        });
        harness.tick();
        assert!(
            composing_rect(&harness).is_some(),
            "precondition: the composing range paints while focused"
        );

        harness.enter_owner_scope(|| {
            FocusManager::global().unfocus();
        });
        harness.tick();

        assert!(
            controller.is_composing(),
            "blur alone must not end the composition itself"
        );
        assert!(
            composing_rect(&harness).is_none(),
            "an unfocused field must stop painting a stale composing underline"
        );
    }

    /// Direct caret navigation (Home, via the ordinary key handler) restores
    /// the caret while the composition itself keeps running — exercised
    /// through the full production key-dispatch path, not just the
    /// controller unit test.
    #[test]
    fn caret_navigation_restores_the_caret_through_the_key_handler_while_composing() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::with_text("abc");
        let mut harness =
            crate::test_harness::mount_with_ime(EditableText::new(controller.clone()));
        let node_id = controller.focus_node_id().expect("published node");
        harness.enter_owner_scope(|| {
            FocusManager::global().request_focus(node_id);
        });
        harness.tick();

        dispatch_ime(&flui_types::ImeEvent::Preedit {
            text: "def".to_string(),
            cursor: None,
        });
        harness.tick();
        assert!(
            !show_caret_flag(&harness),
            "precondition: caret hidden while composing"
        );

        use flui_interaction::events::{Code, KeyState, NamedKey};
        use flui_interaction::testing::input::KeyEventBuilder;
        let home_event = KeyEventBuilder::new(Code::Home)
            .with_key(Key::Named(NamedKey::Home))
            .with_state(KeyState::Down)
            .build();
        let handled = FocusManager::global().dispatch_key_event(&home_event);
        assert!(handled);
        harness.tick();

        assert!(
            show_caret_flag(&harness),
            "moving the caret directly must restore its visibility"
        );
        assert!(
            controller.is_composing(),
            "caret navigation must not end the composition"
        );
    }

    /// The cursor-area loop (ADR-0032, upgraded by ADR-0033) prefers the
    /// composing rect while composing, and falls back to the caret rect
    /// once composition is cancelled.
    #[test]
    fn cursor_area_loop_prefers_the_composing_rect_and_falls_back_to_the_caret_rect_after_cancel() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let mut harness =
            crate::test_harness::mount_with_ime(EditableText::new(controller.clone()));
        let node_id = controller.focus_node_id().expect("published node");

        harness.enter_owner_scope(|| {
            FocusManager::global().request_focus(node_id);
        });
        harness.tick();
        assert_eq!(harness.cursor_area_calls().len(), 1);

        dispatch_ime(&flui_types::ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: Some((2, 2)),
        });
        harness.tick();

        let composing = composing_rect(&harness).expect("an active composing range");
        let sent_while_composing = *harness
            .cursor_area_calls()
            .last()
            .expect("a send while composing");
        assert_eq!(
            sent_while_composing.origin,
            Point::new(composing.left(), composing.top()),
            "the loop must prefer the composing rect while composing"
        );
        assert_eq!(
            sent_while_composing.size,
            flui_types::Size::new(composing.width(), composing.height())
        );

        // Cancel the composition — `Preedit("")`, winit's own signal.
        dispatch_ime(&flui_types::ImeEvent::Preedit {
            text: String::new(),
            cursor: None,
        });
        harness.tick();

        assert!(composing_rect(&harness).is_none());
        let caret = caret_rect(&harness);
        let sent_after_cancel = *harness
            .cursor_area_calls()
            .last()
            .expect("a send after cancel");
        assert_eq!(
            sent_after_cancel.origin,
            Point::new(caret.left(), caret.top()),
            "the loop must fall back to the caret rect once composition ends"
        );
    }
}
