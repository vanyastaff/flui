//! [`EditableText`] against a mounted tree: focus and enablement, submit
//! keys, the IME session and its cursor-area loop, pointer selection, the
//! obscured-text mapping as the render object sees it, and composing-region
//! paint. The key handler, the mask and the render-view assembly stay unit
//! tests in `src/text/editable_text.rs`.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use flui_interaction::events::{Key, KeyState, NamedKey};
use flui_interaction::routing::FocusNode;
use flui_objects::RenderEditable;
use flui_types::{Point, Rect};
use flui_view::prelude::*;
use flui_widgets::__test_access::TextEditingControllerProbe as _;
use flui_widgets::{EditableText, TextEditingController};

/// A field constructed disabled keeps its explicit node attached for
/// lifecycle correctness but refuses focus acquisition.
///
/// Oracle analog: `'Does not accept updates when read-only'`
/// (`editable_text_test.dart`, tag `3.44.0`) — **adapted, not a direct
/// port**: Flutter's `readOnly` blocks platform text updates while the
/// field keeps focus; `enabled` is a strictly wider gate that withholds
/// focus acquisition entirely (see `tests/parity/editable_text_test.rs`'s
/// module doc for the full contrast).
///
/// Red-check: stop forwarding `enabled` into
/// `FocusNode::set_can_request_focus` — the request below lands.
#[test]
fn disabled_field_refuses_focus_on_its_explicit_node() {
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("disabled EditableText");
    let harness = crate::common::harness::mount(
        EditableText::new(controller, Rc::clone(&focus_node)).enabled(false),
    );

    focus_node.request_focus();
    assert!(focus_node.is_attached());
    assert!(!focus_node.has_primary_focus());
    assert!(harness.focus_manager().primary_focus().is_none());
}

/// An enabled field (the default) does publish, so the same field
/// re-enabled is focusable again — the contrast case for the test above.
///
/// Oracle analog: `'Does not accept updates when read-only'`
/// (`editable_text_test.dart`, tag `3.44.0`) — see
/// `disabled_field_refuses_focus_on_its_explicit_node`'s doc comment for the
/// adapted contrast this is the enabled-side counterpart of.
#[test]
fn enabled_field_accepts_focus_on_its_explicit_node() {
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("enabled EditableText");
    let _harness =
        crate::common::harness::mount(EditableText::new(controller, Rc::clone(&focus_node)));

    focus_node.request_focus();
    assert!(focus_node.has_primary_focus());
}

/// Disabling a focused field unfocuses it and withdraws its published
/// node — `did_update_view`'s `set_can_request_focus(false)` call (see
/// [`EditableText::enabled`]'s doc comment) releases primary focus itself,
/// matching Flutter's `FocusNode.canRequestFocus` setter.
///
/// Oracle analog: `'Does not accept updates when read-only'`
/// (`editable_text_test.dart`, tag `3.44.0`) — see
/// `disabled_field_does_not_publish_its_focus_node`'s doc comment above
/// for the adapted contrast (Flutter's `readOnly` keeps focus; `enabled`
/// releases it outright).
///
/// Red-check: pass `true` instead of `new_view.enabled` to
/// `set_can_request_focus` in `did_update_view` — the node stays
/// primary-focused and the first assertion fails.
#[test]
fn disabling_a_focused_field_unfocuses_its_explicit_node() {
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("disable while focused");
    let mut harness = crate::common::harness::mount(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));

    focus_node.request_focus();
    assert!(focus_node.has_primary_focus());

    harness.swap_root(EditableText::new(controller, Rc::clone(&focus_node)).enabled(false));

    assert!(!focus_node.has_primary_focus());
    assert!(!focus_node.can_request_focus());
}

/// The contrast case: re-enabling a disabled field republishes its
/// node, so it becomes focusable again.
///
/// Oracle analog: `'Does not accept updates when read-only'`
/// (`editable_text_test.dart`, tag `3.44.0`) — see
/// `disabled_field_does_not_publish_its_focus_node`'s doc comment for the
/// adapted contrast.
#[test]
fn re_enabling_a_disabled_field_restores_explicit_node_focusability() {
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("re-enabled EditableText");
    let mut harness = crate::common::harness::mount(
        EditableText::new(controller.clone(), Rc::clone(&focus_node)).enabled(false),
    );
    assert!(!focus_node.can_request_focus());

    harness.swap_root(EditableText::new(controller, Rc::clone(&focus_node)));

    assert!(focus_node.can_request_focus());
    focus_node.request_focus();
    assert!(focus_node.has_primary_focus());
}

// ------------------------------------------------------------------
// IME integration
//
// `mount_with_ime` installs a `TextInputHandle` tied to a harness-owned
// `TextInputOwner`. Application tests separately cover a real
// presentation-owned platform capability. These tests dispatch through
// the SAME owner the field attaches to, matching production routing after
// the platform event has been demultiplexed to its presentation.
// ------------------------------------------------------------------

fn dispatch_ime(harness: &crate::common::harness::Harness, event: &flui_types::ImeEvent) {
    harness.dispatch_ime(event);
}

fn mount_ime_field(
    controller: TextEditingController,
    label: &'static str,
) -> (crate::common::harness::Harness, Rc<FocusNode>) {
    let focus_node = FocusNode::with_debug_label(label);
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller,
        Rc::clone(&focus_node),
    ));
    (harness, focus_node)
}

fn character_key_event(ch: char) -> flui_interaction::events::KeyEvent {
    use flui_interaction::events::Code;
    use flui_interaction::testing::input::KeyEventBuilder;
    KeyEventBuilder::new(Code::KeyA)
        .with_key(Key::Character(ch.to_string()))
        .with_state(KeyState::Down)
        .build()
}

/// A named-key event with modifiers, for the Shift-modified arrows.
fn named_key_event(
    named: NamedKey,
    modifiers: flui_interaction::events::Modifiers,
) -> flui_interaction::events::KeyEvent {
    use flui_interaction::events::Code;
    use flui_interaction::testing::input::KeyEventBuilder;
    KeyEventBuilder::new(Code::ArrowRight)
        .with_key(Key::Named(named))
        .with_state(KeyState::Down)
        .with_modifiers(modifiers)
        .build()
}

/// Shift routes an arrow to the EXTEND operation, not the move one.
///
/// The controller has both and they behave differently on the same input;
/// this asserts the key handler picks between them, which is the only
/// thing that makes the extend operations reachable at all.
///
/// Red-check: drop the `modifiers.contains(Modifiers::SHIFT)` branch from
/// the `ArrowRight` arm — the caret moves and the selection stays
/// collapsed.
#[test]
fn shift_routes_an_arrow_to_the_extend_operation() {
    use flui_interaction::events::Modifiers;

    let controller = TextEditingController::with_text("hello world");
    controller.set_caret_byte_offset(0);
    let focus_node = FocusNode::with_debug_label("shift-arrow field");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));
    focus_node.request_focus();

    harness
        .focus_manager()
        .dispatch_key_event(&named_key_event(NamedKey::ArrowRight, Modifiers::SHIFT));

    assert_eq!(
        controller.selection(),
        0..1,
        "Shift+Right must extend, not move"
    );

    // Control: the same key WITHOUT Shift collapses instead.
    harness
        .focus_manager()
        .dispatch_key_event(&named_key_event(NamedKey::ArrowRight, Modifiers::empty()));

    assert!(
        !controller.has_selection(),
        "the unmodified arrow collapses the selection it just made"
    );
    assert_eq!(
        controller.caret_byte_offset(),
        1,
        "and collapses to the span's end, without stepping past it"
    );
}

fn enter_key_event() -> flui_interaction::events::KeyEvent {
    use flui_interaction::events::Code;
    use flui_interaction::testing::input::KeyEventBuilder;
    KeyEventBuilder::new(Code::Enter)
        .with_key(Key::Named(NamedKey::Enter))
        .with_state(KeyState::Down)
        .build()
}

/// Enter, dispatched through a mounted field the way production input
/// arrives (`FocusManager::dispatch_key_event`, not `build_key_handler`
/// called directly), reaches `on_submitted` with the field's current
/// text.
///
/// Red-check: delete the `Key::Named(NamedKey::Enter)` arm — this test
/// then falls through to `Key::Named(_) => Ignored` and the assertion on
/// `submitted.borrow()` fails (still `None`).
#[test]
fn enter_calls_on_submitted_with_the_current_text() {
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("submit field");
    let submitted: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let submitted_for_callback = Rc::clone(&submitted);

    let harness = crate::common::harness::mount(
        EditableText::new(controller.clone(), Rc::clone(&focus_node)).on_submitted(move |text| {
            submitted_for_callback.replace(Some(text.to_string()));
        }),
    );
    focus_node.request_focus();

    harness
        .focus_manager()
        .dispatch_key_event(&character_key_event('h'));
    harness
        .focus_manager()
        .dispatch_key_event(&character_key_event('i'));
    assert_eq!(controller.text(), "hi", "typed text reaches the buffer");
    assert_eq!(*submitted.borrow(), None, "typing alone must not submit");

    let result = harness
        .focus_manager()
        .dispatch_key_event(&enter_key_event());

    assert_eq!(
        *submitted.borrow(),
        Some("hi".to_string()),
        "Enter calls on_submitted with the field's current text"
    );
    assert!(result, "Enter is consumed once a callback is set");
}

/// The contrast case: with no `on_submitted`, Enter is left unconsumed —
/// unchanged from this field's behavior before the callback existed, so
/// an ancestor can still act on a bare Enter press.
#[test]
fn enter_with_no_on_submitted_is_ignored() {
    let controller = TextEditingController::with_text("hi");
    let focus_node = FocusNode::with_debug_label("no-submit field");
    let harness =
        crate::common::harness::mount(EditableText::new(controller, Rc::clone(&focus_node)));
    focus_node.request_focus();

    let result = harness
        .focus_manager()
        .dispatch_key_event(&enter_key_event());

    assert!(!result, "with no on_submitted, Enter is not consumed");
}

/// IME owns Enter while composing — the same suppression contract the
/// `Key::Character` arm already follows. An in-progress composition
/// must not also trigger submit.
///
/// Red-check: delete the `controller.is_composing()` guard — this test
/// then sees `submitted` populated despite the active composition.
#[test]
fn enter_while_composing_is_ignored_and_does_not_submit() {
    let controller = TextEditingController::with_text("hi");
    controller.set_composing_text("に", None);
    let focus_node = FocusNode::with_debug_label("composing field");
    let submitted: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let submitted_for_callback = Rc::clone(&submitted);

    let harness = crate::common::harness::mount(
        EditableText::new(controller.clone(), Rc::clone(&focus_node)).on_submitted(move |text| {
            submitted_for_callback.replace(Some(text.to_string()));
        }),
    );
    focus_node.request_focus();
    assert!(controller.is_composing(), "sanity: composition is active");

    let result = harness
        .focus_manager()
        .dispatch_key_event(&enter_key_event());

    assert!(!result, "Enter must not be consumed while composing");
    assert_eq!(*submitted.borrow(), None, "the IME owns Enter, not submit");
}

/// A command chord is not a submit — mirroring the `Key::Character`
/// arm's own command-chord guard, Ctrl+Enter/Cmd+Enter must bubble to
/// an ancestor `Shortcuts` rather than being swallowed here.
///
/// Red-check: delete the `is_command_chord(event.modifiers)` guard —
/// this test then sees `submitted` populated by a Ctrl+Enter press.
#[test]
fn ctrl_enter_is_a_command_chord_not_a_submit() {
    use flui_interaction::events::{Code, Modifiers};
    use flui_interaction::testing::input::KeyEventBuilder;

    let controller = TextEditingController::with_text("hi");
    let focus_node = FocusNode::with_debug_label("ctrl-enter field");
    let submitted: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let submitted_for_callback = Rc::clone(&submitted);

    let harness = crate::common::harness::mount(
        EditableText::new(controller, Rc::clone(&focus_node)).on_submitted(move |text| {
            submitted_for_callback.replace(Some(text.to_string()));
        }),
    );
    focus_node.request_focus();

    let event = KeyEventBuilder::new(Code::Enter)
        .with_key(Key::Named(NamedKey::Enter))
        .with_state(KeyState::Down)
        .with_modifiers(Modifiers::CONTROL)
        .build();
    let result = harness.focus_manager().dispatch_key_event(&event);

    assert!(!result, "Ctrl+Enter must bubble, not be consumed here");
    assert_eq!(*submitted.borrow(), None, "a command chord must not submit");
}

/// Shift+Enter is reserved for a future multiline newline, not submit —
/// this substrate has no multiline support yet, but the reservation is
/// deliberate.
///
/// Red-check: delete the Shift guard — this test then sees `submitted`
/// populated by a Shift+Enter press.
#[test]
fn shift_enter_is_reserved_and_does_not_submit() {
    use flui_interaction::events::{Code, Modifiers};
    use flui_interaction::testing::input::KeyEventBuilder;

    let controller = TextEditingController::with_text("hi");
    let focus_node = FocusNode::with_debug_label("shift-enter field");
    let submitted: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let submitted_for_callback = Rc::clone(&submitted);

    let harness = crate::common::harness::mount(
        EditableText::new(controller, Rc::clone(&focus_node)).on_submitted(move |text| {
            submitted_for_callback.replace(Some(text.to_string()));
        }),
    );
    focus_node.request_focus();

    let event = KeyEventBuilder::new(Code::Enter)
        .with_key(Key::Named(NamedKey::Enter))
        .with_state(KeyState::Down)
        .with_modifiers(Modifiers::SHIFT)
        .build();
    let result = harness.focus_manager().dispatch_key_event(&event);

    assert!(
        !result,
        "Shift+Enter must not be consumed as a submit today"
    );
    assert_eq!(
        *submitted.borrow(),
        None,
        "reserved for newline, not submit"
    );
}

/// Auto-repeat (macOS/Win32 report a held Enter as repeated `Down`
/// events) must not resubmit on every tick — the key is still consumed,
/// just without calling the callback again.
///
/// Red-check: delete the `!event.repeat` guard — the call counter below
/// reaches 2, not 1.
#[test]
fn repeated_enter_consumes_the_key_without_resubmitting() {
    use flui_interaction::events::Code;
    use flui_interaction::testing::input::KeyEventBuilder;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let controller = TextEditingController::with_text("hi");
    let focus_node = FocusNode::with_debug_label("repeat field");
    let calls = Rc::new(AtomicUsize::new(0));
    let calls_for_callback = Rc::clone(&calls);

    let harness = crate::common::harness::mount(
        EditableText::new(controller, Rc::clone(&focus_node)).on_submitted(move |_text| {
            calls_for_callback.fetch_add(1, Ordering::Relaxed);
        }),
    );
    focus_node.request_focus();

    let initial_press = KeyEventBuilder::new(Code::Enter)
        .with_key(Key::Named(NamedKey::Enter))
        .with_state(KeyState::Down)
        .build();
    let repeated_press = KeyEventBuilder::new(Code::Enter)
        .with_key(Key::Named(NamedKey::Enter))
        .with_state(KeyState::Down)
        .with_repeat(true)
        .build();

    let first = harness.focus_manager().dispatch_key_event(&initial_press);
    assert!(first, "the initial press submits and is consumed");
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    let repeated = harness.focus_manager().dispatch_key_event(&repeated_press);
    assert!(repeated, "a repeat is still consumed");
    assert_eq!(
        calls.load(Ordering::Relaxed),
        1,
        "a repeat must not resubmit"
    );
}

/// The real `examples/todo.rs` path: `on_submitted` calls
/// `TextEditingController::clear()` on (a clone of) the same
/// controller from inside the callback. Proves the callback runs with
/// no outstanding borrow of either `on_submitted` or the outer
/// controller cell that this reentrant call could conflict with.
#[test]
fn on_submitted_may_clear_its_own_controller_without_panicking() {
    let controller = TextEditingController::with_text("hi");
    let focus_node = FocusNode::with_debug_label("clear-on-submit field");
    let controller_for_callback = controller.clone();

    let harness = crate::common::harness::mount(
        EditableText::new(controller.clone(), Rc::clone(&focus_node)).on_submitted(move |_text| {
            controller_for_callback.clear();
        }),
    );
    focus_node.request_focus();

    let result = harness
        .focus_manager()
        .dispatch_key_event(&enter_key_event());

    assert!(result, "Enter is consumed");
    assert_eq!(
        controller.text(),
        "",
        "the callback's own clear() must have taken effect, not panicked"
    );
}

/// A root that can drop its `EditableText`, so a still-focused field can
/// be unmounted out from under its own focus — the dispose-contract
/// scenario. Mirrors `hero_controller::Root`'s show/hide shape.
#[derive(Clone)]
struct ImeUnmountRoot {
    controller: TextEditingController,
    focus_node: Rc<FocusNode>,
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
            EditableText::new(self.controller.clone(), Rc::clone(&self.focus_node)).boxed()
        } else {
            flui_widgets::Text::new("gone").boxed()
        }
    }
}

/// Red-check: skip the `TextInputHandle::attach` call on focus gain (make
/// the IME focus listener a no-op) — this test's `active_count`
/// assertion fails and the later dispatch reaches nobody.
#[test]
fn a_focus_request_queued_before_mount_attaches_the_ime_client() {
    use flui_interaction::routing::FocusRequestOutcome;

    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("pre-mount IME focus");
    assert_eq!(
        focus_node.request_focus(),
        FocusRequestOutcome::Queued,
        "an unattached node retains the request until its widget mounts"
    );

    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));

    assert!(focus_node.has_primary_focus());
    assert_eq!(
        harness.active_ime_clients(),
        1,
        "attach fulfills the queued focus only after the IME listener exists"
    );
    dispatch_ime(&harness, &flui_types::ImeEvent::Commit("x".to_owned()));
    assert_eq!(controller.text(), "x");
}

#[test]
fn swapping_a_focused_node_restarts_exactly_one_ime_session() {
    use flui_interaction::routing::FocusRequestOutcome;

    let controller = TextEditingController::new();
    let first = FocusNode::with_debug_label("first live field node");
    let replacement = FocusNode::with_debug_label("replacement live field node");
    let mut harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&first),
    ));

    first.request_focus();
    assert_eq!(harness.ime_allowed_calls(), [true]);
    assert_eq!(replacement.request_focus(), FocusRequestOutcome::Queued);

    harness.swap_root(EditableText::new(
        controller.clone(),
        Rc::clone(&replacement),
    ));

    assert!(!first.is_attached());
    assert!(replacement.has_primary_focus());
    assert_eq!(harness.active_ime_clients(), 1);
    assert_eq!(
        harness.ime_allowed_calls(),
        [true, false, true],
        "the old focused node detaches once and its queued replacement attaches once"
    );
    dispatch_ime(&harness, &flui_types::ImeEvent::Commit("z".to_owned()));
    assert_eq!(controller.text(), "z");
}

/// Swapping the controller on a live field retargets everything that
/// reaches one — the painted text, IME input, and key input.
///
/// The focus node is deliberately UNCHANGED, so the only difference
/// between the two views is the controller; swapping both would let a
/// node-driven re-registration pass for a controller-driven one.
///
/// Each assertion has its negative half: the replacement receiving the
/// input is not enough, because a field forwarding to BOTH would satisfy
/// it. The original must also stop.
#[test]
fn swapping_the_controller_retargets_paint_ime_and_keys_to_the_replacement() {
    let original = TextEditingController::with_text("original");
    let replacement = TextEditingController::with_text("replacement");
    let focus_node = FocusNode::with_debug_label("controller swap");
    let mut harness = crate::common::harness::mount_with_ime(EditableText::new(
        original.clone(),
        Rc::clone(&focus_node),
    ));
    harness.enter_owner_scope(|| {
        focus_node.request_focus();
    });
    harness.tick();

    let painted_before =
        with_render_editable(&harness, |editable| editable.plain_text().to_string())
            .expect("a mounted EditableText always has a RenderEditable");
    assert_eq!(
        painted_before, "original",
        "premise: the field starts on the original"
    );

    harness.swap_root(EditableText::new(
        replacement.clone(),
        Rc::clone(&focus_node),
    ));
    harness.tick();

    let painted_after =
        with_render_editable(&harness, |editable| editable.plain_text().to_string())
            .expect("a mounted EditableText always has a RenderEditable");
    assert_eq!(
        painted_after, "replacement",
        "the swap must reach the render object; before this the field kept \
             painting the controller it was born with"
    );

    // IME input follows the swap.
    dispatch_ime(&harness, &flui_types::ImeEvent::Commit("z".to_owned()));
    assert_eq!(
        replacement.text(),
        "replacementz",
        "IME commits must reach the replacement"
    );
    assert_eq!(
        original.text(),
        "original",
        "and must NOT reach the original — a field forwarding to both \
             would pass the assertion above"
    );

    // Key input follows the swap.
    let handled = harness
        .focus_manager()
        .dispatch_key_event(&character_key_event('!'));
    assert!(handled, "the focused field must still consume the key");
    assert_eq!(
        replacement.text(),
        "replacementz!",
        "the key handler registered at mount must read the replacement"
    );
    assert_eq!(
        original.text(),
        "original",
        "and must not reach the original"
    );
}

/// Rebuilding with the SAME controller registers no second listener.
///
/// The control for the test above, and it needs an observable the visible
/// text cannot give: a `did_update_view` that retargeted unconditionally
/// would drop and re-add the listener on every rebuild, which paints
/// identically. The first version of this test asserted the text and a
/// tautology (`a.is_same_controller(&a.clone())`, which cannot fail);
/// `listener_count` is what actually pins it.
#[test]
fn rebuilding_with_the_same_controller_registers_no_second_listener() {
    let controller = TextEditingController::with_text("stable");
    let focus_node = FocusNode::with_debug_label("same controller rebuild");
    let mut harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));
    harness.tick();

    let after_mount = controller.listener_count();
    assert!(
        after_mount > 0,
        "premise: a mounted field registers a change listener, got \
             {after_mount}"
    );

    for _ in 0..3 {
        harness.swap_root(EditableText::new(
            controller.clone(),
            Rc::clone(&focus_node),
        ));
        harness.tick();
    }

    assert_eq!(
        controller.listener_count(),
        after_mount,
        "three rebuilds with the same controller must leave the listener \
             count where mounting put it — a retarget that did not check \
             identity would have added three more"
    );
}

/// The change listener MOVES on a swap: off the original, onto the
/// replacement.
///
/// The one part of the retarget that cannot ride on the shared cell,
/// because it is registered ON the controller rather than read FROM it —
/// so it is the part most likely to be forgotten, and the only one with a
/// count to check.
#[test]
fn swapping_the_controller_moves_the_change_listener_rather_than_adding_one() {
    let original = TextEditingController::with_text("original");
    let replacement = TextEditingController::with_text("replacement");
    let focus_node = FocusNode::with_debug_label("listener move");
    let mut harness = crate::common::harness::mount_with_ime(EditableText::new(
        original.clone(),
        Rc::clone(&focus_node),
    ));
    harness.tick();

    let original_after_mount = original.listener_count();
    let replacement_before = replacement.listener_count();
    assert!(
        original_after_mount > 0,
        "premise: the mounted field listens to the original"
    );

    harness.swap_root(EditableText::new(
        replacement.clone(),
        Rc::clone(&focus_node),
    ));
    harness.tick();

    assert_eq!(
        original.listener_count(),
        original_after_mount - 1,
        "the field must DEREGISTER from the original — leaving it \
             attached would keep an unmounted-from controller waking this \
             field for edits it no longer shows"
    );
    assert_eq!(
        replacement.listener_count(),
        replacement_before + 1,
        "and register on the replacement exactly once"
    );
}

/// A normal post-mount focus edge attaches one IME client and routes
/// composition to this field's controller.
#[test]
fn focus_gain_attaches_an_ime_client_and_routes_preedit_to_the_controller() {
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("IME focus gain");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));

    focus_node.request_focus();
    assert_eq!(
        harness.active_ime_clients(),
        1,
        "focus gain must attach an IME client"
    );

    dispatch_ime(
        &harness,
        &flui_types::ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: Some((0, 2)),
        },
    );

    assert_eq!(controller.text(), "ni");
    assert_eq!(controller.composing_range(), Some(0..2));
}

#[test]
fn commit_replaces_the_composing_region_through_the_attached_client() {
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("IME commit");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));
    focus_node.request_focus();

    dispatch_ime(
        &harness,
        &flui_types::ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: Some((2, 2)),
        },
    );
    dispatch_ime(&harness, &flui_types::ImeEvent::Commit("你".to_string()));

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
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("IME composition key");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));
    focus_node.request_focus();

    dispatch_ime(
        &harness,
        &flui_types::ImeEvent::Preedit {
            text: "n".to_string(),
            cursor: Some((1, 1)),
        },
    );
    assert_eq!(controller.text(), "n");

    let handled = harness
        .focus_manager()
        .dispatch_key_event(&character_key_event('n'));
    assert!(handled, "the focused field must still consume the key");
    assert_eq!(
        controller.text(),
        "n",
        "a Key::Character delivered during active composition must not \
             insert on top of the preedit"
    );
}

/// The plain-typing case the suppression guard must not break: IME is
/// attached (the field is focused, `TextInputOwner` has a client) but
/// no preedit is active, so ordinary characters insert exactly as they
/// would with no IME composition involved at all.
#[test]
fn character_key_with_ime_attached_but_no_active_preedit_inserts_normally() {
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("plain key with IME");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));
    focus_node.request_focus();
    assert!(!controller.is_composing(), "precondition: no preedit yet");

    let handled = harness
        .focus_manager()
        .dispatch_key_event(&character_key_event('x'));
    assert!(handled);
    assert_eq!(controller.text(), "x");
}

/// Red-check: drop the `else if let Some(token) = ... handle.detach`
/// branch in the IME focus listener — this test's `active_count`
/// assertion after `unfocus()` fails (stays 1).
#[test]
fn blur_detaches_the_ime_client() {
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("IME blur");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller,
        Rc::clone(&focus_node),
    ));
    focus_node.request_focus();
    assert_eq!(harness.active_ime_clients(), 1);

    harness.focus_manager().unfocus();
    assert_eq!(
        harness.active_ime_clients(),
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
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("IME unmount");
    let mut harness = crate::common::harness::mount_with_ime(ImeUnmountRoot {
        controller: controller.clone(),
        focus_node: Rc::clone(&focus_node),
        show: true,
    });
    focus_node.request_focus();
    assert_eq!(harness.active_ime_clients(), 1);

    harness.swap_root(ImeUnmountRoot {
        controller: controller.clone(),
        focus_node,
        show: false,
    });

    assert_eq!(
        harness.active_ime_clients(),
        0,
        "unmounting a still-focused field must detach its IME client \
             (the ADR-0030 dispose contract)"
    );
}

/// Oracle analog: `'connection is closed when TextInputClient
/// .onConnectionClosed message received'` (`editable_text_test.dart`,
/// tag `3.44.0`) — see `disabled_removes_the_underline_and_restores_the_caret`'s
/// doc comment for the documented divergence (Flutter keeps the buffer;
/// FLUI strips the composing slice).
///
/// Red-check: drop the `guard.text.replace_range(range, "")` in
/// `TextEditingController::clear_composing` (keep only the marker
/// clear) — this test's text assertion fails, keeping the uncommitted
/// preedit instead of stripping it.
#[test]
fn disabled_mid_preedit_strips_the_composing_slice_through_the_attached_client() {
    let controller = TextEditingController::with_text("Hello ");
    let focus_node = FocusNode::with_debug_label("IME disabled");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));
    focus_node.request_focus();

    dispatch_ime(
        &harness,
        &flui_types::ImeEvent::Preedit {
            text: "wor".to_string(),
            cursor: Some((3, 3)),
        },
    );
    assert_eq!(controller.text(), "Hello wor");

    dispatch_ime(&harness, &flui_types::ImeEvent::Disabled);

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
// IME cursor-area tracking (ADR-0030)
//
// `CursorAreaLoop`'s `LocalPostFrameHandle::schedule_local` call
// addresses the harness's lane directly (a `Weak` pointer, minted once
// by `install_build_capabilities`) — it does not need `enter_owner_scope`
// active to succeed, only the lane and its scheduler to still be alive.
// These tests still wrap focusing/blurring in `harness.
// enter_owner_scope(...)` for parity with production's `realm.enter`
// shape, but that wrapping is no longer load-bearing for the loop
// itself; a focus change dispatched outside it starts the loop exactly
// the same way. A focus change with the harness's binding already
// dropped would still attach/detach the IME client correctly (that part
// needs no lane at all), it would just never start the loop — the
// `LocalPostFrameScheduleError::LaneClosed` path `CursorAreaLoop::schedule`
// warns on rather than panicking over.
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
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("cursor area padding");
    let mut harness = crate::common::harness::mount_with_ime(
        flui_widgets::Padding::only(20.0, 10.0, 0.0, 0.0)
            .child(EditableText::new(controller, Rc::clone(&focus_node))),
    );

    harness.enter_owner_scope(|| {
        focus_node.request_focus();
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
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("cursor advance");
    let mut harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));

    harness.enter_owner_scope(|| {
        focus_node.request_focus();
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
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("cursor dedupe");
    let mut harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller,
        Rc::clone(&focus_node),
    ));
    let focus_owner = harness.focus_manager();

    harness.enter_owner_scope(|| {
        focus_node.request_focus();
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
        focus_owner.unfocus();
    });
    harness.tick();
    harness.enter_owner_scope(|| {
        focus_node.request_focus();
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
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("IME enabled");
    let mut harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller,
        Rc::clone(&focus_node),
    ));

    harness.enter_owner_scope(|| {
        focus_node.request_focus();
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

    dispatch_ime(&harness, &flui_types::ImeEvent::Enabled);
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
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("cursor loop blur");
    let mut harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));
    let focus_owner = harness.focus_manager();

    harness.enter_owner_scope(|| {
        focus_node.request_focus();
    });
    harness.tick();
    assert_eq!(harness.cursor_area_calls().len(), 1);

    harness.enter_owner_scope(|| {
        focus_owner.unfocus();
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
/// send-silent and pass a sends-only assertion. This test instead reads
/// the owner-local post-frame lane after each frame: a live loop queues
/// its next firing from inside the current one, so it leaves exactly one
/// callback pending after every frame, and a stopped loop leaves none.
///
/// Red-check: drop the `alive.set(false)` call in `EditableTextState::
/// dispose` — one callback stays pending after the dispose frame and after
/// every subsequent `tick()`.
#[test]
fn loop_stops_rescheduling_after_dispose_while_still_focused() {
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("cursor loop dispose");
    let mut harness = crate::common::harness::mount_with_ime(ImeUnmountRoot {
        controller: controller.clone(),
        focus_node: Rc::clone(&focus_node),
        show: true,
    });

    harness.enter_owner_scope(|| {
        focus_node.request_focus();
    });
    harness.tick();
    assert_eq!(harness.cursor_area_calls().len(), 1);
    let lane = harness.local_post_frame_handle();
    assert_eq!(
        lane.pending_len(),
        1,
        "precondition: the live loop keeps its next firing queued"
    );

    // `swap_root` disposes the old (still-focused) field within its own
    // pumped frame: dispose runs during that frame's build phase,
    // before the SAME frame's post-frame phase drains the `fire()`
    // queued by the `tick()` above — so a correctly stopped loop must
    // not reschedule even once more here.
    harness.swap_root(ImeUnmountRoot {
        controller: controller.clone(),
        focus_node,
        show: false,
    });
    let calls_after_unmount = harness.cursor_area_calls().len();
    assert_eq!(
        lane.pending_len(),
        0,
        "the dispose frame's firing must not queue a successor"
    );

    // Further frames must not resurrect scheduling, and must not panic.
    controller.insert_str("y");
    for frame in 1..=2 {
        harness.tick();
        assert_eq!(
            lane.pending_len(),
            0,
            "a disposed-while-focused field must not keep rescheduling its \
                 cursor-area loop (frame {frame} after dispose) — a zombie loop \
                 would reschedule once per frame forever even though its sends \
                 look silent"
        );
    }

    assert_eq!(
        harness.cursor_area_calls().len(),
        calls_after_unmount,
        "unmounting a still-focused field must not send further cursor \
             areas"
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
    let controller = TextEditingController::new();
    let focus_node = FocusNode::with_debug_label("cursor loop refocus");
    let mut harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller,
        Rc::clone(&focus_node),
    ));
    let focus_owner = harness.focus_manager();

    harness.enter_owner_scope(|| {
        focus_node.request_focus();
    });
    harness.tick();
    assert_eq!(harness.cursor_area_calls().len(), 1);

    harness.enter_owner_scope(|| {
        focus_owner.unfocus();
        focus_node.request_focus();
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
// Composing-region underline + hidden caret (ADR-0030)
// ------------------------------------------------------------------

/// Runs `f` against the mounted field's single `RenderEditable`, found
/// by downcasting the one render object this widget mounts.
fn with_render_editable<T>(
    harness: &crate::common::harness::Harness,
    f: impl FnOnce(&RenderEditable) -> T,
) -> Option<T> {
    let owner = harness.pipeline_owner();
    owner.with(|owner| {
        let tree = owner.render_tree();
        let mut f = Some(f);
        for (_, node) in tree.iter() {
            let editable = node
                .as_box()
                .and_then(|b| b.render_object().downcast_ref::<RenderEditable>()); // test-only reach to the one concrete render object type this widget mounts, through the storage layer's `&dyn RenderObject<BoxProtocol>` erasure — same sanctioned boundary as `CursorAreaLoop::global_caret_rect` above.
            if let Some(editable) = editable {
                return f.take().map(|f| f(editable));
            }
        }
        None
    })
}

/// Whether the mounted field's caret is currently painted.
fn show_caret_flag(harness: &crate::common::harness::Harness) -> bool {
    with_render_editable(harness, RenderEditable::show_caret).unwrap_or(false)
}

/// The mounted field's composing-region rect, if any — `None` covers
/// both "no `RenderEditable` found" and "no composing range active".
fn composing_rect(harness: &crate::common::harness::Harness) -> Option<Rect> {
    with_render_editable(harness, RenderEditable::rect_for_composing_range).flatten()
}

/// The mounted field's collapsed caret rect — always geometry, per
/// [`RenderEditable::caret_local_rect`]'s visibility-independence
/// contract.
fn caret_rect(harness: &crate::common::harness::Harness) -> Rect {
    with_render_editable(harness, RenderEditable::caret_local_rect)
        .expect("a mounted EditableText always has a RenderEditable")
}

/// An obscured field's real characters never reach the render object.
///
/// This is the criterion — "obscured text never leaks through paint,
/// semantics, or diagnostics" — asserted where it is decidable. The
/// substitution happens at the one point the controller's text becomes
/// the render view's, so `RenderEditable::plain_text` is downstream of it
/// and so is everything below: the `TextPainter`, the layer tree, and
/// every diagnostic that renders the tree. Redacting at each of those
/// instead would leave the next one to be remembered.
///
/// The assertion is on the ABSENCE of the plaintext, not merely on the
/// presence of bullets: a mask built beside a still-forwarded original
/// would satisfy the second and fail this.
#[test]
fn an_obscured_field_never_hands_its_real_text_to_the_render_object() {
    let controller = TextEditingController::with_text("hunter2");
    let focus_node = FocusNode::with_debug_label("obscured field");
    let harness = crate::common::harness::mount_with_ime(
        EditableText::new(controller, Rc::clone(&focus_node)).obscure_text(true),
    );

    let painted = with_render_editable(&harness, |editable| editable.plain_text().to_string())
        .expect("a mounted EditableText always has a RenderEditable");

    assert_eq!(
        painted, "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}",
        "seven source characters must reach the render object as seven \
             bullets, got {painted:?}"
    );
    assert!(
        !painted.contains("hunter") && !painted.contains('h') && !painted.contains('2'),
        "no fragment of the plaintext may reach the render object, got \
             {painted:?}"
    );
}

/// The mounted field's selection, as the render object received it.
fn render_selection(harness: &crate::common::harness::Harness) -> Option<Range<usize>> {
    with_render_editable(harness, |editable| editable.selection().cloned()).flatten()
}

/// A tap places the caret where it landed.
///
/// The x is chosen from the field's own geometry rather than guessed: the
/// caret rect after the tap is compared against the caret rect the same
/// offset produces when set programmatically, so the assertion does not
/// depend on this host's font metrics.
///
/// Red-check: drop `controller.set_caret_byte_offset(offset)` from the
/// pointer-down handler — the caret stays at the end, where
/// `with_text` left it.
#[test]
fn a_tap_places_the_caret_where_it_landed() {
    let controller = TextEditingController::with_text("hello world");
    let focus_node = FocusNode::with_debug_label("tapped field");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));
    assert_eq!(
        controller.caret_byte_offset(),
        11,
        "precondition: the caret starts at the end"
    );

    harness.dispatch_pointer_down(1.0, 5.0);

    assert_eq!(
        controller.caret_byte_offset(),
        0,
        "a tap at the left edge belongs before the first character"
    );
    assert!(!controller.has_selection(), "a tap collapses");
}

/// A tap on an unfocused field focuses it, so one gesture both focuses and
/// places the caret — the caret would otherwise be set on a field that
/// then rebuilds without it.
#[test]
fn a_tap_focuses_the_field() {
    let controller = TextEditingController::with_text("hello world");
    let focus_node = FocusNode::with_debug_label("unfocused field");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller,
        Rc::clone(&focus_node),
    ));
    assert!(
        !focus_node.has_primary_focus(),
        "precondition: the field starts unfocused"
    );

    harness.dispatch_pointer_down(1.0, 5.0);

    assert!(focus_node.has_primary_focus());
}

/// A drag selects from where it started to where the pointer is, and the
/// caret follows the pointer rather than the lower end.
///
/// Red-check: drop the `set_selection(from, to)` in the pointer-move
/// handler — the selection stays collapsed at the down position.
#[test]
fn a_drag_selects_from_its_start_to_the_pointer() {
    let controller = TextEditingController::with_text("hello world");
    let focus_node = FocusNode::with_debug_label("dragged field");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));

    harness.dispatch_pointer_down(1.0, 5.0);
    let from = controller.caret_byte_offset();
    harness.dispatch_pointer_move(400.0, 5.0);

    let selection = controller.selection();
    assert_eq!(
        selection.start, from,
        "the anchor stays where the drag began"
    );
    assert!(
        selection.end > from,
        "dragging right must extend the selection, got {selection:?}"
    );
    assert_eq!(
        controller.caret_byte_offset(),
        selection.end,
        "the caret follows the pointer, not the lower end"
    );
}

/// A double-tap selects the whole word under it — the composition
/// `EditableTextState::wrap_double_tap_word_select` adds around
/// `install_pointer_handlers`'s plain tap-places-caret behavior. The
/// first tap alone still just collapses (`Listener` never waits for
/// the arena); the second tap's own DOWN then widens that caret into
/// the enclosing word.
///
/// Red-check: skip wrapping `install_pointer_handlers`'s return value
/// in `wrap_double_tap_word_select` — the selection stays collapsed
/// after the second tap, same as the first.
#[test]
fn a_double_tap_selects_the_word_under_it() {
    let controller = TextEditingController::with_text("hello world");
    let focus_node = FocusNode::with_debug_label("double-tapped field");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));

    // First tap: places a collapsed caret, same as
    // `a_tap_places_the_caret_where_it_landed`.
    harness.dispatch_pointer_down(1.0, 5.0);
    harness.dispatch_pointer_up(1.0, 5.0);
    assert!(
        !controller.has_selection(),
        "the first tap alone only collapses"
    );

    // Second tap, same spot: `on_double_tap_down` widens it to the word.
    harness.dispatch_pointer_down(1.0, 5.0);

    assert_eq!(
        controller.selection(),
        0..5,
        "a double-tap at the start of \"hello\" selects the whole word"
    );
}

/// A word selection made by a double-tap must survive the second
/// contact wobbling before it lifts — near-universal on touch, where a
/// finger is essentially never perfectly still between down and up.
///
/// `install_pointer_handlers`'s own `down` handler ran for this same
/// contact (both `Listener` and `GestureDetector` see every pointer
/// event) and set `drag_anchor` before `on_double_tap_down` had a
/// chance to widen the selection — so without clearing that anchor, the
/// very next move would read it and call
/// `set_selection(anchor, moved_to)`, collapsing the word selection
/// back down to a near-zero-byte range anchored at the tap point.
///
/// Red-check: drop `drag_anchor.set(None)` from
/// `wrap_double_tap_word_select`'s `on_double_tap_down` closure — the
/// selection after the move is a tiny range near the tap point, not
/// `0..5`.
#[test]
fn a_double_tap_selects_the_word_even_if_the_second_contact_moves_before_lifting() {
    let controller = TextEditingController::with_text("hello world");
    let focus_node = FocusNode::with_debug_label("wobbly double-tapped field");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));

    harness.dispatch_pointer_down(1.0, 5.0);
    harness.dispatch_pointer_up(1.0, 5.0);

    harness.dispatch_pointer_down(1.0, 5.0);
    assert_eq!(
        controller.selection(),
        0..5,
        "precondition: the second tap's down already selected the word"
    );

    // The second contact moves by a pixel before lifting.
    harness.dispatch_pointer_move(2.0, 5.0);
    assert_eq!(
        controller.selection(),
        0..5,
        "a stray move on the still-down second contact must not clobber \
             the word selection"
    );

    harness.dispatch_pointer_up(2.0, 5.0);
    assert_eq!(
        controller.selection(),
        0..5,
        "lifting the second contact must not change the selection either"
    );
}

/// A disabled field must not attach `on_double_tap_down` at all —
/// `wrap_double_tap_word_select` skips the builder call entirely
/// rather than attaching it and returning early inside, which is what
/// this test's OBSERVABLE assertion (no selection change) shares with
/// the old, insufficient fix. The reason the distinction matters is
/// structural, not behavioral here: `GestureDetector`'s
/// `RecognizerGroup::double_tap_active` joins the arena whenever the
/// callback SLOT is set, regardless of what the callback does once
/// called — an attached-but-early-returning callback would still hold
/// the shared arena across the double-tap window for every tap on a
/// disabled field, delaying an ancestor's own tap. Proving THAT
/// requires a clock-driven harness this test module does not have
/// (`crate::common::harness::Harness` has no `pump_for`); the mechanism
/// itself — that attaching the slot at all is what makes a detector
/// join the arena — is covered directly at the `GestureDetector`
/// level by `gesture_detector_advanced.rs`'s own participation-gating
/// tests.
#[test]
fn a_disabled_field_does_not_select_on_double_tap() {
    let controller = TextEditingController::with_text("hello world");
    let focus_node = FocusNode::with_debug_label("disabled field");
    let harness = crate::common::harness::mount_with_ime(
        EditableText::new(controller.clone(), Rc::clone(&focus_node)).enabled(false),
    );

    harness.dispatch_pointer_down(1.0, 5.0);
    harness.dispatch_pointer_up(1.0, 5.0);
    harness.dispatch_pointer_down(1.0, 5.0);

    assert!(
        !controller.has_selection(),
        "a disabled field must not select a word on double-tap"
    );
}

/// The field being disabled BETWEEN a first tap's down and up — not
/// disabled for the whole gesture, as the test above covers — must
/// not strand the double-tap recognizer either. The rebuild that
/// flips `enabled` to `false` removes the `on_double_tap_down` slot,
/// so `RecognizerGroup::double_tap_active` stops gating NEW
/// registrations; without `forward` still delivering events to an
/// ALREADY-tracked pointer regardless of that gate, the recognizer's
/// own Up handler would never run, leaving it stuck in `FirstDown`
/// forever — even after re-enabling, since nothing else polls a
/// recognizer out of that phase.
///
/// The completing tap is the SECOND contact's own DOWN, not a third
/// dispatch: once the first tap's Up reaches the recognizer at all
/// (the fix under test), it is already `WaitingForSecond` by the
/// time this re-enables, so the very next down completes the SAME
/// pair. An earlier version of this test dispatched a third down
/// "for a fresh double-tap" — which, precisely BECAUSE the first
/// pair was still live, was actually the second half of a doomed
/// THIRD tap, and its own `Listener`-driven single-tap caret
/// placement collapsed the selection the real double-tap had just
/// made, failing this test for the wrong reason.
///
/// Red-check: re-add `if self.double_tap_active() { ... }` around
/// `RecognizerGroup::forward`'s `self.double_tap.handle_event(...)`
/// call — the second tap's down then starts a fresh `FirstDown`
/// instead of completing the pair, and nothing is selected.
#[test]
fn toggling_disabled_between_the_first_taps_down_and_up_does_not_strand_the_recognizer() {
    let controller = TextEditingController::with_text("hello world");
    let focus_node = FocusNode::with_debug_label("toggled field");
    let mut harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));

    // First tap starts while enabled...
    harness.dispatch_pointer_down(1.0, 5.0);
    // ...the field is disabled before that same contact lifts...
    harness.swap_root(EditableText::new(controller.clone(), Rc::clone(&focus_node)).enabled(false));
    harness.dispatch_pointer_up(1.0, 5.0);
    // ...then re-enabled.
    harness.swap_root(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));

    // The second contact of the SAME pair: the recognizer must still
    // be `WaitingForSecond`, not stuck in `FirstDown`.
    harness.dispatch_pointer_down(1.0, 5.0);

    assert_eq!(
        controller.selection(),
        0..5,
        "the double-tap must still complete after enabled toggled off \
             then on mid-gesture, not strand the recognizer in FirstDown"
    );
}

/// A move with no drag in flight must not anchor a selection at whatever
/// offset was last there. The pointer-up clears the anchor, so a move
/// after it is somebody else's.
///
/// Red-check: drop `drag_anchor.set(None)` from the pointer-up handler —
/// the trailing move extends a selection the user is no longer making.
#[test]
fn a_move_after_the_pointer_is_up_selects_nothing() {
    let controller = TextEditingController::with_text("hello world");
    let focus_node = FocusNode::with_debug_label("released field");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));

    harness.dispatch_pointer_down(1.0, 5.0);
    harness.dispatch_pointer_move(400.0, 5.0);
    assert!(
        controller.has_selection(),
        "precondition: the drag made a selection"
    );
    harness.dispatch_pointer_up(400.0, 5.0);
    let after_release = controller.selection();

    // BACK to where the drag began, not to another point past the text:
    // the first draft moved to x=200, which clamps to the same end offset
    // as x=400, so the assertion held whether or not the anchor was
    // cleared. Returning to the down position is the one move whose
    // effect — collapsing the selection — is unmistakable.
    harness.dispatch_pointer_move(1.0, 5.0);

    assert_eq!(
        controller.selection(),
        after_release,
        "a move after release must change nothing; leaving the drag anchor \
             set would collapse this selection back to its start"
    );
    assert!(
        !after_release.is_empty(),
        "the fixture must have selected something"
    );
}

/// A cancelled gesture abandons its drag, so the NEXT move — which may
/// belong to another gesture entirely — does not extend a selection the
/// user gave up on.
///
/// `pointer_cancel` is documented as "abandon any in-flight tracking";
/// handling only `pointer_up` leaves the anchor set on the one path where
/// no up ever arrives.
///
/// Red-check: drop the `on_pointer_cancel` handler — the trailing move
/// collapses the selection back to the drag's start.
#[test]
fn a_cancelled_gesture_abandons_its_drag() {
    let controller = TextEditingController::with_text("hello world");
    let focus_node = FocusNode::with_debug_label("cancelled field");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller.clone(),
        Rc::clone(&focus_node),
    ));

    harness.dispatch_pointer_down(1.0, 5.0);
    harness.dispatch_pointer_move(400.0, 5.0);
    let at_cancel = controller.selection();
    assert!(
        !at_cancel.is_empty(),
        "precondition: the drag selected something"
    );

    harness.dispatch_pointer_cancel();
    harness.dispatch_pointer_move(1.0, 5.0);

    assert_eq!(
        controller.selection(),
        at_cancel,
        "a move after a cancel must change nothing"
    );
}

/// A disabled field ignores the pointer entirely — no caret, no focus.
#[test]
fn a_disabled_field_ignores_a_tap() {
    let controller = TextEditingController::with_text("hello world");
    let focus_node = FocusNode::with_debug_label("disabled field");
    let harness = crate::common::harness::mount_with_ime(
        EditableText::new(controller.clone(), Rc::clone(&focus_node)).enabled(false),
    );

    harness.dispatch_pointer_down(1.0, 5.0);

    assert_eq!(
        controller.caret_byte_offset(),
        11,
        "the caret must not move on a disabled field"
    );
    assert!(!focus_node.has_primary_focus());
}

/// The controller's selection reaches the render object.
///
/// Without this the highlight `RenderEditable` can paint has no source —
/// correct, tested, unreachable code, which is this repository's most
/// common defect shape.
#[test]
fn the_controllers_selection_reaches_the_render_object() {
    let controller = TextEditingController::with_text("hello world");
    controller.set_selection(6, 11);
    let focus_node = FocusNode::with_debug_label("selecting field");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller,
        Rc::clone(&focus_node),
    ));

    assert_eq!(render_selection(&harness), Some(6..11));
}

/// A collapsed selection arrives as `None`, not as an empty range: that
/// case belongs to the caret, and saying so at the seam is cheaper than
/// relying on the render object to skip it.
#[test]
fn a_collapsed_selection_reaches_the_render_object_as_none() {
    let controller = TextEditingController::with_text("hello world");
    controller.set_caret_byte_offset(4);
    let focus_node = FocusNode::with_debug_label("caret-only field");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller,
        Rc::clone(&focus_node),
    ));

    assert_eq!(render_selection(&harness), None);
}

/// On an obscured field the selection is mapped into MASKED byte space,
/// the same space the text itself is masked into.
///
/// The fixture is chosen so that forwarding the source range **cannot**
/// accidentally produce the right answer. `RenderEditable` clamps
/// whatever it is given to a char boundary of the text it holds, and for
/// masked text those boundaries are multiples of the mask width — so on a
/// short fixture the clamp lands on the correct offsets by coincidence
/// and a mapping bug is invisible. That is what the first draft of this
/// test did, and the mutation run is what exposed it.
///
/// `"aa€bb"` — `a a € b b`, five chars, seven bytes — with the two
/// trailing `b`s selected is source `5..7`. Masked it is the fourth and
/// fifth of five bullets: `9..15`. Forwarding `5..7` unmapped clamps to
/// `6..9`, the *third* bullet. The three answers are pairwise distinct.
///
/// Red-check: pass `controller.selection()` straight through in
/// `build_field_view`.
#[test]
fn an_obscured_fields_selection_is_mapped_into_masked_space() {
    let controller = TextEditingController::with_text("aa€bb");
    controller.set_selection(5, 7);
    let focus_node = FocusNode::with_debug_label("obscured selecting field");
    let harness = crate::common::harness::mount_with_ime(
        EditableText::new(controller, Rc::clone(&focus_node)).obscure_text(true),
    );

    assert_eq!(
        render_selection(&harness),
        Some(9..15),
        "the last two bullets; forwarding the source range unmapped would \
             clamp to 6..9, the third"
    );
}

/// A double-tap on an obscured field must select against the SOURCE
/// text's own cluster widths, not the masked string's uniform ones.
///
/// `source_offset_for_masked_offset`'s own doc spells out the
/// contract: it walks `source`'s grapheme clusters to answer a SOURCE
/// byte offset from a masked cluster INDEX. Passing the masked string
/// itself as `source` (as `source_word_range_at_global` briefly did)
/// makes it walk the masked string's own uniform-width clusters
/// instead — for source `"hello"` (5 one-byte ASCII characters) with
/// the 3-byte default bullet, double-tapping the FIRST bullet then
/// resolved a masked word range of `0..3` (one bullet) and mapped
/// each endpoint independently against the MASKED string, landing on
/// SOURCE `0..3` (the buffer's first three bytes) instead of the one
/// character `0..1` ("h") the tap actually landed on.
///
/// Red-check: swap `source_word_range_at_global`'s `source_text`
/// argument back to `editable.plain_text()` — the selection becomes
/// `0..3` instead of `0..1`.
#[test]
fn a_double_tap_on_an_obscured_field_selects_against_the_source_text() {
    let controller = TextEditingController::with_text("hello");
    let focus_node = FocusNode::with_debug_label("obscured double-tapped field");
    let harness = crate::common::harness::mount_with_ime(
        EditableText::new(controller.clone(), Rc::clone(&focus_node)).obscure_text(true),
    );

    // First tap lands on the first (bullet-masked) character.
    harness.dispatch_pointer_down(1.0, 5.0);
    harness.dispatch_pointer_up(1.0, 5.0);
    harness.dispatch_pointer_down(1.0, 5.0);

    assert_eq!(
        controller.selection(),
        0..1,
        "one source character (\"h\"), not the buffer's first three \
             bytes worth of masked-cluster width"
    );
}

/// The same field WITHOUT the flag hands its text through unchanged.
///
/// The control for the test above: without it, a mask applied
/// unconditionally — or a field that rendered nothing at all — would look
/// identical from the assertion's side.
#[test]
fn a_plain_field_still_hands_its_real_text_to_the_render_object() {
    let controller = TextEditingController::with_text("hunter2");
    let focus_node = FocusNode::with_debug_label("plain field");
    let harness = crate::common::harness::mount_with_ime(EditableText::new(
        controller,
        Rc::clone(&focus_node),
    ));

    let painted = with_render_editable(&harness, |editable| editable.plain_text().to_string())
        .expect("a mounted EditableText always has a RenderEditable");

    assert_eq!(
        painted, "hunter2",
        "an unobscured field is unchanged by this feature"
    );
}

/// `Preedit { cursor: None }` while focused hides the caret and starts
/// painting the composing underline — the FLUI expression of Flutter's
/// `buildTextSpan`'s composing-underline three-way split plus its
/// hidden-caret case, both now implemented (ADR-0030).
///
/// Oracle: `'Composing text is underlined and underline is cleared when
/// losing focus'` (`editable_text_test.dart`, tag `3.44.0`) — ported
/// geometry-relative, see `tests/parity/editable_text_test.rs`'s module
/// doc for why (no `TextStyle.decoration` to source real underline
/// metrics from).
///
/// Red-check: drop the `!controller.caret_hidden_by_ime()` term from
/// `build_field_view`'s `show_caret` expression — this test's
/// `show_caret_flag` assertion fails (stays `true`).
#[test]
fn preedit_cursor_none_while_focused_hides_the_caret_and_starts_the_underline() {
    let controller = TextEditingController::new();
    let (mut harness, focus_node) = mount_ime_field(controller, "preedit hidden caret");
    harness.enter_owner_scope(|| {
        focus_node.request_focus();
    });
    harness.tick();
    assert!(
        show_caret_flag(&harness),
        "precondition: the caret paints while focused with no composition"
    );

    dispatch_ime(
        &harness,
        &flui_types::ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: None,
        },
    );
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
    let controller = TextEditingController::new();
    let (mut harness, focus_node) = mount_ime_field(controller, "preedit visible caret");
    harness.enter_owner_scope(|| {
        focus_node.request_focus();
    });
    harness.tick();

    dispatch_ime(
        &harness,
        &flui_types::ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: Some((2, 2)),
        },
    );
    harness.tick();

    assert!(
        show_caret_flag(&harness),
        "cursor: Some must leave the caret visible"
    );
    assert!(composing_rect(&harness).is_some());
}

/// A commit ends composition: the underline disappears and the caret is
/// restored.
///
/// Oracle: `'Composing text is underlined and underline is cleared when
/// losing focus'` (`editable_text_test.dart`, tag `3.44.0`) — the
/// composition-ends-so-underline-clears half; see
/// `preedit_cursor_none_while_focused_hides_the_caret_and_starts_the_underline`'s
/// doc comment for the geometry-relative adaptation note.
#[test]
fn commit_removes_the_underline_and_restores_the_caret() {
    let controller = TextEditingController::new();
    let (mut harness, focus_node) = mount_ime_field(controller, "commit restores caret");
    harness.enter_owner_scope(|| {
        focus_node.request_focus();
    });
    harness.tick();

    dispatch_ime(
        &harness,
        &flui_types::ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: None,
        },
    );
    harness.tick();
    assert!(
        !show_caret_flag(&harness),
        "precondition: caret hidden while composing"
    );

    dispatch_ime(&harness, &flui_types::ImeEvent::Commit("你".to_string()));
    harness.tick();

    assert!(
        composing_rect(&harness).is_none(),
        "a commit must remove the composing underline"
    );
    assert!(show_caret_flag(&harness), "a commit must restore the caret");
}

/// `Disabled` mid-composition (winit's connection-closed signal) also
/// ends composition: underline gone, caret restored.
///
/// Oracle analog: `'connection is closed when TextInputClient
/// .onConnectionClosed message received'` (`editable_text_test.dart`,
/// tag `3.44.0`) — **adapted, not a direct port**: Flutter's
/// `connectionClosed` only ends the input session, leaving the buffer
/// untouched; FLUI's `ImeEvent::Disabled` additionally strips the
/// in-progress composing slice (see
/// `disabled_mid_preedit_strips_the_composing_slice_through_the_attached_client`
/// below, and `TextEditingController::clear_composing`'s doc, for the
/// documented divergence this pins).
#[test]
fn disabled_removes_the_underline_and_restores_the_caret() {
    let controller = TextEditingController::with_text("Hello ");
    let (mut harness, focus_node) = mount_ime_field(controller, "disabled restores caret");
    harness.enter_owner_scope(|| {
        focus_node.request_focus();
    });
    harness.tick();

    dispatch_ime(
        &harness,
        &flui_types::ImeEvent::Preedit {
            text: "wor".to_string(),
            cursor: None,
        },
    );
    harness.tick();
    assert!(!show_caret_flag(&harness));

    dispatch_ime(&harness, &flui_types::ImeEvent::Disabled);
    harness.tick();

    assert!(composing_rect(&harness).is_none());
    assert!(show_caret_flag(&harness));
}

/// `Preedit("")` — winit's composition-cancel signal — ends composition
/// through the full attached-client path: underline gone, caret
/// restored, and plain typing works immediately after.
#[test]
fn empty_preedit_cancels_the_composition_through_the_attached_client() {
    let controller = TextEditingController::new();
    let (mut harness, focus_node) = mount_ime_field(controller.clone(), "empty preedit");
    harness.enter_owner_scope(|| {
        focus_node.request_focus();
    });
    harness.tick();

    dispatch_ime(
        &harness,
        &flui_types::ImeEvent::Preedit {
            text: "nihao".to_string(),
            cursor: Some((5, 5)),
        },
    );
    harness.tick();
    assert_eq!(controller.text(), "nihao");

    dispatch_ime(
        &harness,
        &flui_types::ImeEvent::Preedit {
            text: String::new(),
            cursor: None,
        },
    );
    harness.tick();

    assert_eq!(controller.text(), "");
    assert!(!controller.is_composing());
    assert!(composing_rect(&harness).is_none());
    assert!(show_caret_flag(&harness));

    let handled = harness
        .focus_manager()
        .dispatch_key_event(&character_key_event('x'));
    assert!(handled);
    assert_eq!(
        controller.text(),
        "x",
        "plain typing must work immediately after the cancel"
    );
}

/// Inactive empty `Preedit` through the attached client must not delete
/// a committed selection — the production path that X11 Start uses
/// (`ImeEvent` → `apply_ime_event` → `set_composing_text`).
#[test]
fn empty_preedit_with_no_composition_preserves_selection_through_attached_client() {
    let controller = TextEditingController::with_text("hello world");
    controller.set_selection(0, 5);
    let (mut harness, focus_node) = mount_ime_field(controller.clone(), "inactive empty preedit");
    harness.enter_owner_scope(|| {
        focus_node.request_focus();
    });
    harness.tick();

    dispatch_ime(
        &harness,
        &flui_types::ImeEvent::Preedit {
            text: String::new(),
            cursor: None,
        },
    );
    harness.tick();

    assert_eq!(controller.text(), "hello world");
    assert_eq!(controller.selection(), 0..5);
    assert_eq!(controller.caret_byte_offset(), 5);
    assert!(!controller.is_composing());
    assert!(composing_rect(&harness).is_none());
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
    let controller = TextEditingController::new();
    let (mut harness, focus_node) = mount_ime_field(controller.clone(), "unfocus composition");
    let focus_owner = harness.focus_manager();
    harness.enter_owner_scope(|| {
        focus_node.request_focus();
    });
    harness.tick();

    dispatch_ime(
        &harness,
        &flui_types::ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: Some((2, 2)),
        },
    );
    harness.tick();
    assert!(
        composing_rect(&harness).is_some(),
        "precondition: the composing range paints while focused"
    );

    harness.enter_owner_scope(|| {
        focus_owner.unfocus();
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
    let controller = TextEditingController::with_text("abc");
    let (mut harness, focus_node) = mount_ime_field(controller.clone(), "caret navigation");
    harness.enter_owner_scope(|| {
        focus_node.request_focus();
    });
    harness.tick();

    dispatch_ime(
        &harness,
        &flui_types::ImeEvent::Preedit {
            text: "def".to_string(),
            cursor: None,
        },
    );
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
    let handled = harness.focus_manager().dispatch_key_event(&home_event);
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

/// The cursor-area loop (ADR-0030) prefers the
/// composing rect while composing, and falls back to the caret rect
/// once composition is cancelled.
#[test]
fn cursor_area_loop_prefers_the_composing_rect_and_falls_back_to_the_caret_rect_after_cancel() {
    let controller = TextEditingController::new();
    let (mut harness, focus_node) = mount_ime_field(controller, "composing cursor area");

    harness.enter_owner_scope(|| {
        focus_node.request_focus();
    });
    harness.tick();
    assert_eq!(harness.cursor_area_calls().len(), 1);

    dispatch_ime(
        &harness,
        &flui_types::ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: Some((2, 2)),
        },
    );
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
    dispatch_ime(
        &harness,
        &flui_types::ImeEvent::Preedit {
            text: String::new(),
            cursor: None,
        },
    );
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

/// `on_changed` reports the user's edits and only those: typing, deleting
/// and an IME commit call it with the new text; a caret move, the caller's
/// own `set_text`, and the controller edit an `on_submitted` callback makes
/// do not.
///
/// Red-check: drop the `EditObserver` around the key handler — nothing is
/// recorded for the typed keys.
#[test]
fn on_changed_reports_user_edits_but_not_the_callers_own() {
    use flui_interaction::events::Modifiers;

    let controller = TextEditingController::new();
    let changes: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&changes);
    let submit_controller = controller.clone();
    let focus_node = FocusNode::with_debug_label("on_changed field");
    let harness = crate::common::harness::mount_with_ime(
        EditableText::new(controller.clone(), Rc::clone(&focus_node))
            .on_changed(move |text| sink.borrow_mut().push(text.to_owned()))
            .on_submitted(move |_| submit_controller.clear()),
    );
    harness.enter_owner_scope(|| focus_node.request_focus());

    let keys = harness.focus_manager();
    keys.dispatch_key_event(&character_key_event('a'));
    keys.dispatch_key_event(&character_key_event('b'));
    keys.dispatch_key_event(&named_key_event(NamedKey::Backspace, Modifiers::empty()));
    dispatch_ime(&harness, &flui_types::ImeEvent::Commit("c".to_string()));
    assert_eq!(*changes.borrow(), ["a", "ab", "a", "ac"]);

    keys.dispatch_key_event(&named_key_event(NamedKey::ArrowLeft, Modifiers::empty()));
    controller.set_text("programmatic");
    keys.dispatch_key_event(&enter_key_event());
    assert_eq!(
        controller.text(),
        "",
        "the submit callback cleared the field"
    );
    assert_eq!(
        *changes.borrow(),
        ["a", "ab", "a", "ac"],
        "a caret move, the caller's set_text and the submit callback's clear are not user edits"
    );
}
