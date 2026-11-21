//! The keys a focused field handles: the chord classification, the key handler
//! that edits the controller, and the actions that keep activation and dismissal
//! keys for the field.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use flui_foundation::TextDirection;
use flui_interaction::focus::{FocusNode, KeyEventHandler, KeyEventResult};
use flui_platform::TargetPlatform;
use flui_platform::input::keyboard::{
    ImeComposition, Key, KeyPress, KeyState, Modifiers, NamedKey,
};
use flui_platform::text::store::{EnterAction, TextInputOptions};
use flui_view::prelude::*;

use super::SubmitCallback;
use crate::interaction::actions::{
    Action, ActionOutcome, ActivateIntent, ActivationPressIntent, DismissIntent,
};
use crate::support::command_modifiers;
use crate::text::controller::TextEditingController;

/// The keys a field types or ignores that the default bindings would turn
/// into a command for an enclosing control: the release of a typed space
/// (`ActivateIntent`), Space's key-down and an Escape. A focused field
/// consumes them without effect, so typing a space in a field inside a
/// pressable card does not press the card, and Escape in a field closes
/// nothing.
#[derive(Clone, Copy)]
pub(super) struct KeptByTheField;

impl Action<ActivateIntent> for KeptByTheField {
    fn invoke(&self, _ctx: &mut EventContext<'_>, _intent: &ActivateIntent) -> ActionOutcome {
        ActionOutcome::Performed
    }
}

impl Action<ActivationPressIntent> for KeptByTheField {
    fn invoke(&self, _: &mut EventContext<'_>, _: &ActivationPressIntent) -> ActionOutcome {
        ActionOutcome::Performed
    }
}

impl Action<DismissIntent> for KeptByTheField {
    fn invoke(&self, _ctx: &mut EventContext<'_>, _intent: &DismissIntent) -> ActionOutcome {
        ActionOutcome::Performed
    }
}

/// Whether these modifiers make a key a *command* rather than *text*.
///
/// Meta (Cmd) always is. Control is, except in combination with Alt: Win32
/// can report AltGr as Control+Alt without explicit AltGraph. Retain that
/// conservative text fallback when the source cannot distinguish the chord.
/// Explicit AltGraph has already excluded Control/Alt from these command
/// dimensions; Meta still has priority.
///
/// The cost of that carve-out, stated rather than discovered: a genuine
/// Ctrl+Alt+X shortcut is delivered as text in a focused field. An additional
/// genuine Control alongside explicit AltGraph also keeps glyph interpretation;
/// the raw event remains available to authored handlers.
#[inline]
pub(super) fn is_command_chord(modifiers: Modifiers) -> bool {
    modifiers.contains(Modifiers::META)
        || (modifiers.contains(Modifiers::CONTROL) && !modifiers.contains(Modifiers::ALT))
}

/// The modifier that requests WORD-granularity caret/selection/delete
/// movement on `platform` — the modifier differs per platform, it is not the
/// same one everywhere:
///
/// | Platform | Word-jump modifier | Why not the other one too |
/// |---|---|---|
/// | macOS, iOS | Alt (Option) | Ctrl is unbound for word-jump on macOS |
/// | Windows, Linux, Android, Fuchsia, Unknown | Control | Alt+Left/Right/Backspace are reserved for LINE-boundary intents this crate does not implement yet (see [`is_word_jump_modifier`]'s `# DEFERRED`); treating Alt as word-jump here too would silently claim that reservation early |
///
/// A pure function of `platform`, table-tested against every
/// [`TargetPlatform`] variant so the mapping itself is verified
/// regardless of which host actually runs the test suite (this crate's
/// tests run on `ubuntu-latest` in CI, which alone would never exercise
/// the macOS/iOS arm).
#[inline]
pub(super) fn word_jump_modifier(platform: TargetPlatform) -> Modifiers {
    match platform {
        TargetPlatform::MacOS | TargetPlatform::iOS => Modifiers::ALT,
        _ => Modifiers::CONTROL,
    }
}

/// Whether these modifiers request WORD-granularity movement on
/// `platform` — see [`word_jump_modifier`]'s doc for the table.
///
/// # Platform source
///
/// `platform` is the [`PlatformConvention`](flui_view::environment::PlatformConvention)
/// the field or area followed at its last build, which the runtime installs
/// from the platform its runner names. The web runner names the compile
/// target for now, not the browser's OS (`docs/gaps.md`).
///
/// # DEFERRED
///
/// Alt+Left/Right/Backspace as a line-boundary intent on non-Apple
/// platforms (extend-selection-to-line-break and delete-to-line-break). Not
/// reinterpreted as word-jump, so a later line-boundary implementation is not
/// fighting an existing, wrong meaning for the chord. Meanwhile Alt+Left and
/// Alt+Right without Shift are returned unhandled ([`is_reserved_alt_arrow`]),
/// so Alt+Left reaches the route's Back binding; with Shift they extend by a
/// character, and Alt+Backspace deletes one. The line-boundary change decides
/// on which platforms it takes the chord back from Back.
///
/// This receives the same command projection as character handling, so explicit
/// AltGraph's Control/Alt aliases cannot request word movement or deletion.
///
/// # Exact chord, not just "the modifier is held"
///
/// Requires EXACTLY the platform's word-jump modifier among
/// {Ctrl, Alt, Meta} — Shift composes independently (it selects
/// move-vs-extend, handled by the caller) and is not part of this check,
/// but any OTHER command modifier held at the same time disqualifies the
/// chord. The complete modifier state is matched (apart from Shift), and
/// without this a chord that
/// is not meant to be word-jump at all — Ctrl+Alt+Right on Linux,
/// Option+Command+Right on macOS — would wrongly take the word-jump path
/// just because it happens to also hold the required key. Lock-state
/// flags, Fn and Symbol were excluded by the command projection; an incidental
/// Caps Lock does not disqualify word movement.
#[inline]
pub(crate) fn is_word_jump_modifier(modifiers: Modifiers, platform: TargetPlatform) -> bool {
    let required = word_jump_modifier(platform);
    [Modifiers::CONTROL, Modifiers::ALT, Modifiers::META]
        .into_iter()
        .all(|modifier| modifiers.contains(modifier) == required.contains(modifier))
}

/// Whether an arrow chord is left to the enclosing route: exactly Alt, no
/// Shift, outside Apple. See [`is_word_jump_modifier`]'s `# DEFERRED`.
#[inline]
pub(super) fn is_reserved_alt_arrow(modifiers: Modifiers, platform: TargetPlatform) -> bool {
    !matches!(platform, TargetPlatform::MacOS | TargetPlatform::iOS)
        && modifiers.contains(Modifiers::ALT)
        && [Modifiers::SHIFT, Modifiers::CONTROL, Modifiers::META]
            .into_iter()
            .all(|modifier| !modifiers.contains(modifier))
}

/// Build the key-event handler closure for `controller`.
///
/// Only `KeyState::Down` events (which cover key-repeat) are acted upon, and
/// only while `focus_node` still allows focus — kept in sync with
/// `EditableText::enabled` by `did_update_view` — so input is ignored on a
/// field disabled after it was focused.
/// The key handler reads its controller through the shared cell at DISPATCH
/// time, not at registration time.
///
/// That is what makes a controller swap take effect on a mounted field: the
/// registration installed at `init_state` outlives the swap, so a handler
/// holding a clone would keep typing into the controller the field was born
/// with.
pub(super) fn build_key_handler(
    controller: Rc<RefCell<TextEditingController>>,
    focus_node: Rc<FocusNode>,
    on_submitted: Rc<RefCell<Option<SubmitCallback>>>,
    text_input: Rc<Cell<TextInputOptions>>,
    writer: WriterSource,
    platform: Rc<Cell<TargetPlatform>>,
    direction: Rc<Cell<TextDirection>>,
) -> KeyEventHandler {
    Rc::new(move |event| {
        // Cloned out, so no borrow of the cell is held while the edit's
        // listeners run: one may rebuild the field onto another controller.
        let controller = controller.borrow().clone();
        // The platform the field followed at its last build.
        let platform = platform.get();
        let direction = direction.get();
        if !focus_node.can_request_focus() {
            return KeyEventResult::Ignored;
        }
        if !matches!(event.state, KeyState::Down(_)) || event.composition == ImeComposition::Active
        {
            return KeyEventResult::Ignored;
        }
        let modifiers = command_modifiers(event.modifiers);
        match &event.key {
            // A command chord is not text. Leave it unconsumed so
            // `FocusManager::dispatch_key_event`'s leaf->root walk reaches the
            // enclosing `Shortcuts`/`CallbackShortcuts` — consuming it here is
            // what makes Ctrl+S type "s" and every app shortcut dead while a
            // field is focused. The clipboard chords take this path too:
            // `DefaultFocusTraversal` binds them, and its `Shortcuts` resolves
            // them against the clipboard actions this field records on its
            // own node.
            //
            // Scoped to character insertion ON PURPOSE. The named-key arms
            // below have no ancestor to fall through to: the default bindings
            // cover no caret-movement intent, so guarding them would not route
            // Ctrl+Home somewhere better — it would make it a no-op. Widen
            // this together with the binding that would answer them.
            Key::Character(_) if is_command_chord(modifiers) => KeyEventResult::Ignored,
            Key::Character(character_string) => {
                // Suppression contract (`ImeEvent`'s doc): suppress
                // `Key::Character` insertion ONLY while a composition is
                // active. A host routes the keys composition consumes
                // as `Preedit` and `Commit`, so this path
                // exists mainly for backends/tests that dispatch a
                // character key mid-preedit anyway — without the guard it
                // would double-insert alongside the IME's own commit.
                // Consumed either way: this field owns the key while
                // focused, composing or not.
                if !controller.is_composing() {
                    controller.insert_str(character_string.as_str());
                }
                KeyEventResult::Handled
            }
            // The platform's word-jump modifier + Backspace deletes the
            // WORD behind the caret rather than one character — see
            // `is_word_jump_modifier`'s doc for which modifier that is on
            // this platform.
            Key::Named(NamedKey::Backspace) => {
                if is_word_jump_modifier(modifiers, platform) {
                    controller.delete_word_backward();
                } else {
                    controller.backspace();
                }
                KeyEventResult::Handled
            }
            // Ctrl/Alt+Delete is the forward mirror of the Backspace arm
            // above.
            Key::Named(NamedKey::Delete) => {
                if is_word_jump_modifier(modifiers, platform) {
                    controller.delete_word_forward();
                } else {
                    controller.delete_forward();
                }
                KeyEventResult::Handled
            }
            // Outside Apple, Alt+Left and Alt+Right are no field motion while
            // the line-boundary intent they are reserved for is absent: the
            // key goes on, and Alt+Left reaches the route's `BackIntent`.
            Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowRight)
                if is_reserved_alt_arrow(modifiers, platform) =>
            {
                KeyEventResult::Ignored
            }
            // Shift is the difference between MOVING the caret and EXTENDING
            // the selection, and the two are not the same operation with a
            // flag: unmodified, an arrow collapses a selection to its edge and
            // stops there; modified, it steps the extent from wherever it is
            // and leaves the anchor. Ctrl/Alt raises
            // the granularity from character to WORD without changing that
            // axis — the two modifiers compose independently.
            //
            // The controller steps by offset: its `left` is toward the text's
            // start. Left and Right name sides of the line, so under a
            // right-to-left line, whose start is on the right, Left steps
            // toward the end. Inside a run against the line's direction the
            // step stays by offset.
            Key::Named(arrow @ (NamedKey::ArrowLeft | NamedKey::ArrowRight)) => {
                let toward_start =
                    (*arrow == NamedKey::ArrowLeft) == (direction == TextDirection::Ltr);
                let extend = modifiers.contains(Modifiers::SHIFT);
                let by_word = is_word_jump_modifier(modifiers, platform);
                match (toward_start, by_word, extend) {
                    (true, true, true) => controller.extend_selection_word_left(),
                    (true, true, false) => controller.move_caret_word_left(),
                    (true, false, true) => controller.extend_selection_left(),
                    (true, false, false) => controller.move_caret_left(),
                    (false, true, true) => controller.extend_selection_word_right(),
                    (false, true, false) => controller.move_caret_word_right(),
                    (false, false, true) => controller.extend_selection_right(),
                    (false, false, false) => controller.move_caret_right(),
                }
                KeyEventResult::Handled
            }
            Key::Named(NamedKey::Home) => {
                if modifiers.contains(Modifiers::SHIFT) {
                    controller.extend_selection_home();
                } else {
                    controller.move_caret_home();
                }
                KeyEventResult::Handled
            }
            Key::Named(NamedKey::End) => {
                if modifiers.contains(Modifiers::SHIFT) {
                    controller.extend_selection_end();
                } else {
                    controller.move_caret_end();
                }
                KeyEventResult::Handled
            }
            // Only claimed when something actually consumes it — with no
            // `on_submitted` set and no focus-moving label, Enter is left
            // `Ignored` so an ancestor can still act on it. Every host
            // delivers the on-screen keyboard's action key as Enter (see
            // `on_submitted`'s doc).
            Key::Named(NamedKey::Enter) => {
                // IME owns Enter while composing — same suppression
                // contract the `Key::Character` arm above follows: an
                // in-progress composition must not also trigger submit.
                if controller.is_composing() {
                    return KeyEventResult::Ignored;
                }
                // A command chord is not a submit, mirroring the
                // `Key::Character` arm's own command-chord guard — without
                // this, Ctrl+Enter/Cmd+Enter would be swallowed here
                // instead of reaching an ancestor `Shortcuts`.
                if is_command_chord(modifiers) {
                    return KeyEventResult::Ignored;
                }
                // Shift+Enter is reserved for a future multiline newline,
                // not submit — this substrate has no multiline support yet
                // (see the type doc's `# DEFERRED (v1)` list), so today
                // this is simply `Ignored`, but the reservation is
                // deliberate: a later multiline field must not find
                // Shift+Enter's meaning already claimed by submit.
                if modifiers.contains(Modifiers::SHIFT) {
                    return KeyEventResult::Ignored;
                }
                // Clone the callback and drop the borrow before calling
                // it — `on_submitted.borrow()` (this statement's own
                // temporary) and the outer `controller` `Ref` are both
                // live at this point, and the callback is arbitrary user
                // code that may itself call back into this same
                // `EditableText` (an `on_submitted` callback that calls
                // `TextEditingController::clear()`). Calling it while either guard
                // is still held is the same drop-under-guard hazard as for a
                // `Mutex`/`RwLock`, applied here to a `RefCell`.
                let callback = on_submitted.borrow().clone();
                // The label the key showed, read before the callback, which
                // may rebuild the field with another.
                let action = text_input.get().enter_action;
                let moves_focus = matches!(action, EnterAction::Next | EnterAction::Previous);
                if callback.is_none() && !moves_focus {
                    return KeyEventResult::Ignored;
                }
                // Auto-repeat (macOS/Win32 report a held Enter as repeated
                // `Down` events, not one Down followed by held state) must
                // not resubmit or move on every tick — the key is still
                // consumed (`Handled`), just without acting again.
                if event.state == KeyState::Down(KeyPress::AutoRepeat) {
                    return KeyEventResult::Handled;
                }
                if let Some(callback) = callback {
                    let text = controller.text();
                    drop(controller);
                    writer.write(|ctx| callback(ctx, &text));
                }
                // A callback that moved the focus itself, or let the field
                // go, decided where it is: the label moves it only from here.
                if moves_focus && focus_node.has_primary_focus() {
                    // Nowhere to go leaves the focus here.
                    if action == EnterAction::Next {
                        focus_node.next_focus();
                    } else {
                        focus_node.previous_focus();
                    }
                }
                KeyEventResult::Handled
            }
            _ => KeyEventResult::Ignored,
        }
    })
}
