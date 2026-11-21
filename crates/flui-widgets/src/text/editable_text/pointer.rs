//! The pointer handlers of a field: a tap places the caret, a drag extends the
//! selection and a double tap selects a word.

use std::{cell::Cell, rc::Rc};

use flui_foundation::geometry::Offset;
use flui_foundation::panic::FirstPanic;
use flui_interaction::events::PointerButton;
use flui_interaction::events::PointerEventExt;
use flui_interaction::pointer::HitTestBehavior;
use flui_interaction::pointer::PointerDispatch;
use flui_platform::text::store::OwnerCalls;
use flui_view::prelude::*;

use super::obscuring::{source_offset_at_global, source_word_range_at_global};
use super::{EditableText, EditableTextState, KeyboardRequest, SelectionDrag};

impl EditableTextState {
    /// Attach the pointer handlers that turn a tap into a caret and a drag
    /// into a selection.
    ///
    /// Split out of `build` only for size; it runs on every rebuild and holds
    /// no state of its own beyond `drag_anchor`, shared with
    /// [`Self::wrap_double_tap_word_select`] (see its doc).
    ///
    /// # What is deliberately absent
    ///
    /// Shift-click extension and triple-tap line selection. Each needs a
    /// click-count or a modifier this level does not see — `Listener`
    /// delivers raw pointer events, and the tap/multi-tap arbitration that
    /// produces those lives in `flui-interaction`'s recognisers. Double-tap
    /// word selection lives one level up, in
    /// [`Self::wrap_double_tap_word_select`], composed around this
    /// method's own return value rather than added here.
    pub(super) fn install_pointer_handlers(
        &self,
        field: crate::interaction::Listener,
        view: &EditableText,
        drag_anchor: Rc<Cell<Option<SelectionDrag>>>,
    ) -> crate::interaction::Listener {
        let enabled = view.enabled;
        let obscuring = view.obscure_text.then_some(view.obscuring_character);
        let owner = self.pipeline_owner.clone();
        let anchor = self.inner_anchor.clone();
        let controller = Rc::clone(&self.controller);
        let focus_node = Rc::clone(&self.focus_node);
        // The first active contact owns `drag_anchor`, in SOURCE byte
        // space, shared with `wrap_double_tap_word_select` (this method's
        // caller passes the SAME cell to both). `None` means no drag of
        // ours is in flight, which is what makes a move that started
        // outside this field — or one that arrived after a cancel, OR one
        // silenced by a double-tap widening this same contact into a word
        // selection (see `wrap_double_tap_word_select`'s doc) — a no-op
        // rather than a selection anchored at whatever was last there.

        let resolve = {
            let controller = Rc::clone(&controller);
            move |global: Offset<f64>| -> Option<usize> {
                // Owned, not borrowed across the call: `source_text` must be
                // the CONTROLLER's source string, not `RenderEditable::
                // plain_text()` (masked on an obscured field) — see
                // `source_offset_at_global`'s doc for why passing the wrong
                // one silently corrupts every obscured-field tap.
                let source_text = controller.borrow().text();
                source_offset_at_global(owner.as_ref()?, &anchor, global, obscuring, &source_text)
            }
        };

        let down = {
            let resolve = resolve.clone();
            let controller = Rc::clone(&controller);
            let focus_node = Rc::clone(&focus_node);
            let drag_anchor = Rc::clone(&drag_anchor);
            let commit = self.composition_commit();
            let paste = self.clipboard_action();
            let handles = Rc::clone(&self.touch_handles);
            move |_ctx: &mut EventContext<'_>, dispatch: PointerDispatch<'_>| {
                let flui_interaction::events::PointerEvent::Down(press) = dispatch.global else {
                    return;
                };
                let Some(position) = dispatch.global.position() else {
                    return;
                };
                // A mouse, pen or trackpad press ends a touch's handles.
                if dispatch.global.pointer_kind()
                    != Some(flui_platform::input::pointer::PointerKind::Touch)
                {
                    let mut failure = FirstPanic::new();
                    handles.hide(&mut failure);
                    failure.resume();
                }
                if !enabled || drag_anchor.get().is_some() {
                    return;
                }
                if press.button() == PointerButton::SECONDARY {
                    // The popup opens when this press's secondary tap wins the
                    // arena (`wrap_double_tap_word_select`), so an enclosing
                    // menu's tap can't open its own over it.
                    handles.menu().secondary_down(position, resolve(position));
                    return;
                }
                if press.button() == PointerButton::AUXILIARY {
                    // A middle press pastes the primary selection where it
                    // lands, on the platforms that have one, and selects
                    // nothing; elsewhere it is left to whatever else uses it.
                    if !paste.primary.is_available() {
                        return;
                    }
                    let Some(at) = resolve(position) else {
                        return;
                    };
                    let mut calls = OwnerCalls::new();
                    calls.run(|| {
                        let _ = focus_node.request_focus();
                    });
                    paste.paste_primary(&mut calls, at);
                    calls.resume();
                    return;
                }
                // Admit the contact before any owner code runs (the commit's
                // `on_changed`, focus observers): a press they dispatch finds
                // this one recorded and is refused, and a reentrant
                // disablement, controller replacement or cancel retires it,
                // which the identity check below sees. The anchor is resolved
                // again once the commit has laid the text out as committed.
                let Some(provisional) = resolve(position) else {
                    return;
                };
                let admitted = SelectionDrag {
                    contact: press.pointer.id,
                    source_anchor: provisional,
                    touch: dispatch.global.pointer_kind()
                        == Some(flui_platform::input::pointer::PointerKind::Touch),
                };
                drag_anchor.set(Some(admitted));
                // A composition in this field is committed before the caret
                // moves (ADR-0142 item 4); the press is still handled when
                // the commit's owner code fails, and that failure is resumed
                // after it.
                let mut calls = OwnerCalls::new();
                commit.run(&mut calls);
                calls.run(|| {
                    if drag_anchor.get() != Some(admitted) {
                        return;
                    }
                    let Some(offset) = resolve(position) else {
                        drag_anchor.set(None);
                        return;
                    };
                    let drag = SelectionDrag {
                        source_anchor: offset,
                        ..admitted
                    };
                    drag_anchor.set(Some(drag));
                    let _ = focus_node.request_focus();
                    if drag_anchor.get() == Some(drag) {
                        controller.borrow().set_caret_byte_offset(offset);
                    }
                });
                calls.resume();
            }
        };

        let moved = {
            let controller = Rc::clone(&controller);
            let drag_anchor = Rc::clone(&drag_anchor);
            move |_ctx: &mut EventContext<'_>, dispatch: PointerDispatch<'_>| {
                let Some(drag) = drag_anchor.get() else {
                    return;
                };
                if flui_interaction::events::PointerEventExt::pointer_id(dispatch.global)
                    != Some(drag.contact)
                {
                    return;
                }
                let Some(to) = dispatch.global.position().and_then(&resolve) else {
                    return;
                };
                // The anchor stays where the drag began; the caret follows the
                // pointer, including backwards. `set_selection` is a no-op
                // when neither moved, which a move stream reports constantly.
                controller.borrow().set_selection(drag.source_anchor, to);
            }
        };

        // Up and cancel do the same thing, and cancel is not optional: it is
        // documented as "abandon any in-flight tracking", and a drag anchor
        // left set after one makes the NEXT move — which may belong to another
        // gesture entirely — extend a selection the user abandoned.
        let release = {
            let drag_anchor = Rc::clone(&drag_anchor);
            let handles = Rc::clone(&self.touch_handles);
            move |_: &mut EventContext<'_>, dispatch: PointerDispatch<'_>| {
                let Some(drag) = drag_anchor.get().filter(|drag| {
                    Some(drag.contact)
                        == flui_interaction::events::PointerEventExt::pointer_id(dispatch.global)
                }) else {
                    return;
                };
                drag_anchor.set(None);
                // A touch that tapped or dragged in the field leaves the
                // handles of what it placed: the caret's or the selection's.
                if drag.touch {
                    let mut failure = FirstPanic::new();
                    handles.show_for_touch(false, &mut failure);
                    failure.resume();
                }
            }
        };
        let cancel = {
            let drag_anchor = Rc::clone(&drag_anchor);
            move |_: &mut EventContext<'_>, dispatch: PointerDispatch<'_>| {
                if drag_anchor.get().is_some_and(|drag| {
                    Some(drag.contact)
                        == flui_interaction::events::PointerEventExt::pointer_id(dispatch.global)
                }) {
                    drag_anchor.set(None);
                }
            }
        };

        field
            .on_pointer_down(down)
            .on_pointer_move(moved)
            .on_pointer_up(release)
            .on_pointer_cancel(cancel)
    }

    /// Wrap `child` (the pointer-handled field this method's caller just
    /// built) in a [`crate::interaction::GestureDetector`] that extends the
    /// selection to the enclosing word on double-tap.
    ///
    /// `GestureDetector` sits OUTSIDE [`crate::interaction::Listener`], not
    /// the other way round: `Listener` never enters the gesture arena
    /// (its own doc explains why), so it keeps placing the caret on every
    /// tap — including the second one of a double-tap — with nothing
    /// competing for that contact. `GestureDetector`'s
    /// `DoubleTapGestureRecognizer` independently watches the SAME pointer
    /// stream and, once it confirms two taps, widens that already-placed
    /// caret into a word selection. Reimplementing double-tap timing/slop
    /// detection by hand inside `Listener`'s handlers instead of composing
    /// the real recognizer would duplicate
    /// `flui_interaction::gesture::DoubleTapGestureRecognizer` rather than reuse
    /// it — see this crate's `ARCHITECTURE.md` Mapping decision for this
    /// composition.
    ///
    /// `on_double_tap_down`, not `on_double_tap`: the word selection should
    /// land as soon as the second tap is confirmed, the same instant
    /// `Listener`'s own `down` handler already placed the caret there —
    /// waiting for the second contact to also lift (`on_double_tap`) would
    /// put the caret and the word-select visibly out of sync for the
    /// gesture's duration.
    ///
    /// `drag_anchor` (the SAME cell `install_pointer_handlers` writes) is
    /// cleared here after the word selection lands. `Listener`'s own `down`
    /// handler already ran for this same contact (both layers see every
    /// pointer event — see this method's `GestureDetector`-vs-`Listener`
    /// doc above) and set the anchor to place the caret, and a touch
    /// contact is essentially never perfectly still: the next `move`, still
    /// on the same still-down second tap, would otherwise read that anchor
    /// and call `set_selection(anchor, moved_to)` — collapsing the word
    /// selection this method just made back down to a near-zero-byte range
    /// anchored at the tap point. Clearing it makes that move a no-op
    /// (`install_pointer_handlers`'s `moved` returns early with no anchor),
    /// exactly like a move that arrived after a cancel.
    pub(super) fn wrap_double_tap_word_select(
        &self,
        child: impl IntoView,
        view: &EditableText,
        gestures: bool,
        drag_anchor: Rc<Cell<Option<SelectionDrag>>>,
    ) -> impl IntoView {
        let obscuring = view.obscure_text.then_some(view.obscuring_character);
        let owner = self.pipeline_owner.clone();
        let anchor = self.inner_anchor.clone();
        let controller = Rc::clone(&self.controller);
        let handles = Rc::clone(&self.touch_handles);

        let mut detector =
            crate::interaction::GestureDetector::new().behavior(HitTestBehavior::Opaque);
        // Attach the callback ONLY while enabled, rather than always
        // attaching it and returning early inside — `on_double_tap_down`
        // being set at all is what makes `RecognizerGroup::double_tap_active`
        // join the arena for a contact (see its own doc). A disabled field
        // is documented to ignore pointer input entirely; leaving the
        // callback attached would still hold the shared arena across the
        // double-tap window for every tap on a disabled field, delaying an
        // ancestor's own tap and letting this no-op recognizer compete to
        // win a contact it does nothing with. `gestures` is `enabled` less a
        // field whose selection gestures the platform owns.
        if gestures {
            detector = detector.on_double_tap_down(move |_ctx, details| {
                let Some(owner) = owner.as_ref() else {
                    return;
                };
                let source_text = controller.borrow().text();
                let Some(range) = source_word_range_at_global(
                    owner,
                    &anchor,
                    details.global_position,
                    obscuring,
                    &source_text,
                ) else {
                    return;
                };
                controller.borrow().set_selection(range.start, range.end);
                drag_anchor.set(None);
                if details.kind == flui_platform::input::pointer::PointerKind::Touch {
                    let mut failure = FirstPanic::new();
                    handles.show_for_touch(false, &mut failure);
                    failure.resume();
                }
            });
        }
        // A tap the arena gives the field (not a drag, a scroll or a press on
        // something drawn over it) asks for the soft keyboard while the field
        // holds the input.
        if gestures && self.ime_handle.is_some() {
            let keyboard = KeyboardRequest {
                handle: self.ime_handle.clone(),
                token: Rc::clone(&self.ime_token),
            };
            detector = detector.on_tap_up(move |_ctx, _details| keyboard.run());
        }
        // The popup opens on the secondary tap a press in the field began,
        // through the arena (see `menu::with_secondary_tap`).
        if gestures && self.touch_handles.menu().look().is_some() {
            let field = (Rc::clone(&self.controller), Rc::clone(&self.focus_node));
            let menu = Rc::clone(self.touch_handles.menu());
            detector =
                super::menu::with_secondary_tap(detector, menu, field, self.composition_commit());
        }
        detector.child(child)
    }
}
