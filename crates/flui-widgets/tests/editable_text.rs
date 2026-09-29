//! [`EditableText`] against a mounted tree: focus and enablement, submit
//! keys, the IME session and its cursor-area loop, pointer selection, the
//! obscured-text mapping as the render object sees it, and composing-region
//! paint. The key handler, the mask and the render-view assembly stay unit
//! tests in `src/text/editable_text.rs`.

use std::rc::Rc;

use flui_interaction::events::{Key, KeyState};
use flui_interaction::routing::FocusNode;
use flui_objects::RenderEditable;
use flui_widgets::{EditableText, TextEditingController};

// ------------------------------------------------------------------
// IME integration
//
// `mount_with_ime` installs a `TextInputHandle` tied to a harness-owned
// `TextInputOwner`. Application tests separately cover a real
// presentation-owned platform capability. These tests dispatch through
// the SAME owner the field attaches to, matching production routing after
// the platform event has been demultiplexed to its presentation.
// ------------------------------------------------------------------

fn dispatch_ime(harness: &crate::common::harness::Harness, event: &flui_platform_api::ImeEvent) {
    harness.dispatch_ime(event);
}

fn character_key_event(ch: char) -> flui_interaction::events::KeyEvent {
    use flui_interaction::events::Code;
    use flui_interaction::testing::input::KeyEventBuilder;
    KeyEventBuilder::new(Code::KeyA)
        .with_key(Key::Character(ch.to_string()))
        .with_state(KeyState::Down)
        .build()
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
        &flui_platform_api::ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: Some((0, 2)),
        },
    );

    assert_eq!(controller.text(), "ni");
    assert_eq!(controller.composing_range(), Some(0..2));
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

// ------------------------------------------------------------------
// The field as a text store (ADR-0090)
//
// The conformance kit (`text_store_kit.rs`) certifies the contract; these
// pin what is specific to `EditableText`: how the store's UTF-16 surface
// maps onto the controller, and when the field and the platform hear of
// each other's changes.
// ------------------------------------------------------------------

mod text_store {
    use std::cell::RefCell;
    use std::rc::Rc;

    use flui_interaction::routing::FocusNode;
    use flui_platform_api::text_store::{
        LockGrant, LockOutcome, LockTiming, Selection, TextStore, TextStoreEdit, TextStoreRead,
        Utf16Offset,
    };
    use flui_widgets::{EditableText, TextEditingController};

    use super::{character_key_event, dispatch_ime};
    use crate::common::harness::{Harness, mount_with_ime};

    /// "a", a supplementary emoji, "e" plus a combining acute, a ZWJ family
    /// and a flag: every offset kind the UTF-16 surface has to map.
    const CORPUS: &str = "a😀e\u{301}👨‍👩‍👧🇯🇵";

    fn at(units: usize) -> Utf16Offset {
        Utf16Offset::new(units)
    }

    fn focused(controller: &TextEditingController) -> (Harness, Rc<FocusNode>) {
        let focus_node = FocusNode::with_debug_label("text store field");
        let mut harness = mount_with_ime(EditableText::new(
            controller.clone(),
            Rc::clone(&focus_node),
        ));
        focus_node.request_focus();
        harness.tick();
        (harness, focus_node)
    }

    fn store(harness: &Harness) -> Rc<dyn TextStore> {
        harness
            .active_text_store()
            .expect("the focused field is the active IME client")
    }

    fn read<R: 'static>(
        store: &Rc<dyn TextStore>,
        body: impl FnOnce(&dyn TextStoreRead) -> R + 'static,
    ) -> R {
        let slot = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&slot);
        let outcome = store.request_lock(
            LockGrant::read(move |session| *sink.borrow_mut() = Some(body(session))),
            LockTiming::Sync,
        );
        assert_eq!(outcome, Ok(LockOutcome::Granted));
        slot.take().expect("the grant ran")
    }

    fn edit<R: 'static>(
        store: &Rc<dyn TextStore>,
        body: impl FnOnce(&mut dyn TextStoreEdit) -> R + 'static,
    ) -> R {
        let slot = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&slot);
        let outcome = store.request_lock(
            LockGrant::read_write(move |session| *sink.borrow_mut() = Some(body(session))),
            LockTiming::Sync,
        );
        assert_eq!(outcome, Ok(LockOutcome::Granted));
        slot.take().expect("the grant ran")
    }

    /// The store's UTF-16 offsets and the controller's UTF-8 bytes name the
    /// same positions, in both directions.
    #[test]
    fn store_offsets_match_controller_bytes_across_surrogates_and_graphemes() {
        let controller = TextEditingController::with_text(CORPUS);
        let (harness, _focus) = focused(&controller);
        let store = store(&harness);

        // Platform to controller: UTF-16 13 is the flag's start, byte 26.
        let set = edit(&store, |session| {
            session.set_selection(Selection {
                anchor: at(13),
                active: at(1),
            })
        });
        assert_eq!(set, Ok(()));
        assert_eq!(controller.selection(), 1..26);
        assert_eq!(
            controller.caret_byte_offset(),
            1,
            "the active end is the caret"
        );

        // Controller to platform: bytes 5..8 ("e" and its mark) are 3..5.
        controller.set_selection(5, 8);
        let seen = read(&store, |session| session.selection());
        assert_eq!(
            seen,
            Selection {
                anchor: at(3),
                active: at(5)
            }
        );
        assert_eq!(read(&store, |session| session.document_len()), at(17));
    }

    /// Red-check: drop `run_deferred_before_app_edit` from the key handler —
    /// the key lands while the commit is still queued and the text reads "b".
    #[test]
    fn typing_after_a_deferred_commit_lands_after_the_commit() {
        let controller = TextEditingController::new();
        let (harness, _focus) = focused(&controller);

        harness.set_transaction_open(true);
        dispatch_ime(
            &harness,
            &flui_platform_api::ImeEvent::Commit("A".to_owned()),
        );
        assert_eq!(controller.text(), "", "the commit waits for the anchor");
        harness.set_transaction_open(false);

        let handled = harness
            .focus_manager()
            .dispatch_key_event(&character_key_event('b'));
        assert!(handled);
        assert_eq!(controller.text(), "Ab");
    }
}

/// Event context (ADR-0086): `on_changed` and `on_submitted` run inside a
/// write the field opens from the writer source it acquired in
/// `init_state`, whichever path made the edit.
mod event_cx {
    use std::rc::Rc;

    use flui_interaction::routing::FocusNode;
    use flui_view::prelude::*;
    use flui_widgets::{EditableText, TextEditingController};

    use super::character_key_event;
    use crate::common::harness::{Harness, mount_with_ime};
    use crate::common::{ProbeSignals, SignalProbe};

    /// A focused field built by `field`, below a probe.
    fn mounted(
        field: impl Fn(ProbeSignals, TextEditingController, Rc<FocusNode>) -> EditableText + 'static,
    ) -> (SignalProbe, Harness, TextEditingController) {
        let controller = TextEditingController::new();
        let focus_node = FocusNode::with_debug_label("event field");
        let (probe_controller, probe_node) = (controller.clone(), Rc::clone(&focus_node));
        let probe = SignalProbe::new(move |signals| {
            field(signals, probe_controller.clone(), Rc::clone(&probe_node))
        });
        let mut harness = mount_with_ime(probe.view());
        harness.enter_owner_scope(|| focus_node.request_focus());
        harness.tick();
        (probe, harness, controller)
    }

    #[test]
    fn typing_writes_through_on_changed_and_rebuilds_its_reader() {
        let (probe, mut harness, _controller) = mounted(|signals, controller, node| {
            let count = signals.count;
            EditableText::new(controller, node)
                .on_changed(move |cx, text| count.set(cx, text.len() as u32))
        });

        let keys = harness.focus_manager();
        keys.dispatch_key_event(&character_key_event('a'));
        keys.dispatch_key_event(&character_key_event('b'));

        assert_eq!(probe.value(), Ok(2));
        harness.tick();
        assert_eq!(probe.reads().last(), Some(&2), "the reader rebuilt");
    }

    #[test]
    fn a_refused_write_in_on_changed_is_reported_not_panicked() {
        let (probe, harness, controller) = mounted(|signals, controller, node| {
            let released = signals.released;
            EditableText::new(controller, node).on_changed(move |cx, _text| released.set(cx, 1))
        });

        let (_, log) = flui_testing::log_capture::capture(|| {
            harness
                .focus_manager()
                .dispatch_key_event(&character_key_event('a'))
        });

        assert!(
            log.contains("an event callback's signal write was refused"),
            "the refusal is logged at the dispatch boundary: {log}"
        );
        assert_eq!(controller.text(), "a", "the edit itself landed");
        assert_eq!(probe.value(), Ok(0));
    }
}
