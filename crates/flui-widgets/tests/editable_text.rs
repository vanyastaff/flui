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
pub(crate) fn focus_gain_attaches_an_ime_client_and_routes_preedit_to_the_controller() {
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
pub(crate) fn an_obscured_field_never_hands_its_real_text_to_the_render_object() {
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
pub(crate) fn a_tap_places_the_caret_where_it_landed() {
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
pub(crate) fn a_drag_selects_from_its_start_to_the_pointer() {
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
pub(crate) fn a_double_tap_selects_the_word_under_it() {
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

/// The arrow keys stop on the grapheme boundaries the painter clusters text
/// by and snaps a tap to (`flui_painting::text_boundaries`, ICU4X): right
/// arrow from the start stops at each cluster's end, left arrow from the
/// end at each cluster's start, and one Backspace removes one cluster. The
/// texts hold a ZWJ family, two regional-indicator flags, stacked combining
/// marks, a CR LF and the Devanagari conjunct "क्षि", one cluster since
/// UAX #29's conjunct rule (GB9c). Fails if the editor clusters by another
/// segmenter that splits any of them differently: then a caret can stop
/// where a tap never lands, or a Backspace leaves part of what was drawn as
/// one character.
pub(crate) fn the_editor_steps_the_graphemes_the_painter_snaps_to() {
    use flui_painting::text_boundaries::graphemes;

    let texts = [
        "a\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F466}b",
        "\u{1F1FA}\u{1F1F8}\u{1F1EB}\u{1F1F7}",
        "e\u{301}\u{302}x",
        "a\r\nb",
        "\u{915}\u{94D}\u{937}\u{93F}",
    ];
    let mut failures = Vec::new();
    for text in texts {
        let clusters: Vec<_> = graphemes(text).collect();
        let controller = TextEditingController::with_text(text);

        controller.set_caret_byte_offset(0);
        let mut right = Vec::new();
        for _ in 0..text.len() {
            controller.move_caret_right();
            right.push(controller.caret_byte_offset());
            if controller.caret_byte_offset() == text.len() {
                break;
            }
        }
        let ends: Vec<usize> = clusters.iter().map(|cluster| cluster.end).collect();

        controller.set_caret_byte_offset(text.len());
        let mut left = Vec::new();
        for _ in 0..text.len() {
            controller.move_caret_left();
            left.push(controller.caret_byte_offset());
            if controller.caret_byte_offset() == 0 {
                break;
            }
        }
        let starts: Vec<usize> = clusters.iter().rev().map(|cluster| cluster.start).collect();

        let mut backspaces = 0;
        while !controller.text().is_empty() && backspaces <= text.len() {
            controller.move_caret_end();
            controller.backspace();
            backspaces += 1;
        }

        if right != ends || left != starts || backspaces != clusters.len() {
            failures.push(format!(
                "{text:?}: right {right:?} vs {ends:?}, left {left:?} vs {starts:?}, \
                 {backspaces} backspaces for {} clusters",
                clusters.len()
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

// ------------------------------------------------------------------
// The field as a text store (ADR-0090)
//
// The conformance kit (`text_store_kit.rs`) certifies the contract; these
// pin what is specific to `EditableText`: how the store's UTF-16 surface
// maps onto the controller, and when the field and the platform hear of
// each other's changes.
// ------------------------------------------------------------------

pub(crate) mod text_store {
    use std::cell::RefCell;
    use std::rc::Rc;

    use flui_interaction::routing::FocusNode;
    use flui_platform_api::text_store::{
        LockGrant, LockOutcome, LockTiming, Selection, TextStore, TextStoreEdit, TextStoreRead,
        Utf16Offset,
    };
    use flui_widgets::{EditableText, TextEditingController};

    use super::character_key_event;
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

    pub(crate) fn rtl_scalar_rect_midpoints_resolve_to_the_source_scalar() {
        use flui_foundation::geometry::Point;
        use flui_platform_api::text_store::{PointMode, Utf16Range};
        let controller = TextEditingController::with_text("אב");
        let (harness, _focus) = focused(&controller);
        let field = store(&harness);
        read(&field, |session| {
            for scalar in 0..2 {
                let from = session
                    .rect_for_range(Utf16Range::collapsed(at(scalar)))
                    .expect("laid-out scalar caret")
                    .bounds;
                let to = session
                    .rect_for_range(Utf16Range::collapsed(at(scalar + 1)))
                    .expect("laid-out next scalar caret")
                    .bounds;
                assert!(from.origin.x > to.origin.x, "Hebrew source carets descend");
                let point = Point::new(
                    from.origin.x.midpoint(to.origin.x),
                    from.origin.y + from.size.height / 2.0,
                );
                assert_eq!(
                    session.index_at_point(point, PointMode::Exact),
                    Ok(at(scalar))
                );
                assert_eq!(
                    session.index_at_point(from.origin, PointMode::Nearest),
                    Ok(at(scalar))
                );
            }
        });
    }

    /// The store's UTF-16 offsets and the controller's UTF-8 bytes name the
    /// same positions, in both directions.
    pub(crate) fn store_offsets_match_controller_bytes_across_surrogates_and_graphemes() {
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

    /// A commit the input method sent inside a frame is queued; when that
    /// frame fails before its commit anchor, the commit is still queued as
    /// the next key arrives, and the key lands after it.
    ///
    /// Red-check: drop `run_deferred_before_app_edit` from the key handler —
    /// the key lands while the commit is still queued and the text reads "b".
    pub(crate) fn typing_after_a_deferred_commit_lands_after_the_commit() {
        let controller = TextEditingController::new();
        let (mut harness, _focus) = focused(&controller);
        let field = store(&harness);
        let text_in_frame = Rc::new(RefCell::new(None));
        let (observed_controller, observed_text) = (controller.clone(), Rc::clone(&text_in_frame));
        harness
            .local_post_frame_handle()
            .schedule_local(move |_| {
                let outcome = flui_platform_api::text_store::project_ime_event(
                    &*field,
                    &flui_platform_api::ImeEvent::Commit("A".to_owned()),
                );
                assert_eq!(outcome, Ok(LockOutcome::Deferred));
                *observed_text.borrow_mut() = Some(observed_controller.text());
                panic!("the frame fails after the commit was queued");
            })
            .expect("post-frame handle installed");

        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| harness.tick()));
        assert!(failed.is_err(), "the failing frame is raised");
        assert_eq!(
            text_in_frame.borrow().as_deref(),
            Some(""),
            "inside the frame the commit waits for the anchor"
        );
        assert_eq!(
            controller.text(),
            "",
            "the failed frame never reached its anchor, so the commit is still queued"
        );

        let handled = harness
            .focus_manager()
            .dispatch_key_event(&character_key_event('b'));
        assert!(handled);
        assert_eq!(controller.text(), "Ab");
    }

    /// Text entered through the focus manager remains editable in a narrow
    /// viewport: geometry, candidate placement and pointer insertion agree.
    pub(crate) fn long_input_reveals_the_caret_and_maps_visible_pointer_positions() {
        crate::common::cases::run_cases(
            "long input viewport",
            &[
                ("latin input", long_latin_input as fn()),
                ("rtl input", long_rtl_input),
                ("obscured input", long_obscured_input),
            ],
        );
    }

    fn long_latin_input() {
        long_input("abcdefghijklmnopqrstuvwxyz", false);
    }

    fn long_rtl_input() {
        long_input("אבגדהוזחטיכלמנסעפצקרשת", false);
    }

    fn long_obscured_input() {
        long_input("abcdefghijklmnopqrstuvwxyz", true);
    }

    fn long_input(text: &'static str, obscured: bool) {
        use flui_foundation::geometry::Point;
        use flui_interaction::events::{Code, Key, KeyState, Modifiers, NamedKey};
        use flui_interaction::testing::input::KeyEventBuilder;
        use flui_platform_api::text_store::{PointMode, TextStoreError, Utf16Range};
        use flui_widgets::SizedBox;

        let controller = TextEditingController::new();
        let focus = FocusNode::new();
        let mut harness = crate::common::harness::mount_with_ime(SizedBox::new(60.0, 30.0).child(
            EditableText::new(controller.clone(), Rc::clone(&focus)).obscure_text(obscured),
        ));
        focus.request_focus();
        for ch in text.chars() {
            assert!(
                harness
                    .focus_manager()
                    .dispatch_key_event(&super::character_key_event(ch))
            );
            harness.tick();
        }
        assert_eq!(controller.text(), text);
        let field = store(&harness);
        let assert_visible = |harness: &Harness| {
            let caret = controller.caret_byte_offset();
            let units =
                flui_platform_api::text_store::utf16::utf16_offset(&controller.text(), caret)
                    .expect("controller caret is a scalar boundary");
            let rect = read(&field, move |session| {
                session
                    .rect_for_range(Utf16Range::collapsed(units))
                    .expect("laid-out caret")
                    .bounds
            });
            assert!(
                rect.origin.x >= -0.001 && rect.origin.x + rect.size.width <= 60.001,
                "active caret must stay inside the field for {text:?}: {rect:?}"
            );
            read(&field, move |session| {
                let mut hidden = 0;
                for scalar in 0..session.document_len().get() {
                    let from = session
                        .rect_for_range(Utf16Range::collapsed(at(scalar)))
                        .expect("scalar geometry")
                        .bounds;
                    let to = session
                        .rect_for_range(Utf16Range::collapsed(at(scalar + 1)))
                        .expect("next scalar geometry")
                        .bounds;
                    let x = from.origin.x.midpoint(to.origin.x);
                    if !(0.0..60.0).contains(&x) {
                        hidden += 1;
                        let point = Point::new(x, from.origin.y + from.size.height / 2.0);
                        assert_eq!(
                            session.index_at_point(point, PointMode::Exact),
                            Err(TextStoreError::PointOutside),
                            "an offscreen glyph must not answer an exact viewport query"
                        );
                    }
                }
                assert!(hidden > 0, "long input has offscreen glyphs");
            });
            let candidate = harness
                .cursor_area_calls()
                .last()
                .copied()
                .expect("candidate area reported");
            assert!(
                candidate.origin.x >= -0.001 && candidate.origin.x + candidate.size.width <= 60.001,
                "IME candidate tracks the visible caret: {candidate:?}"
            );
            rect
        };
        assert_visible(&harness);
        let units = flui_platform_api::text_store::utf16::utf16_len(text);
        let full = read(&field, move |session| {
            session
                .rect_for_range(Utf16Range::new(at(0), units).expect("ordered document range"))
                .expect("laid-out document")
        });
        assert!(full.clipped, "long document exceeds the visible field");
        for key in [NamedKey::Home, NamedKey::End, NamedKey::Home, NamedKey::End] {
            let event = KeyEventBuilder::new(Code::Home)
                .with_key(Key::Named(key))
                .with_state(KeyState::Down)
                .with_modifiers(Modifiers::empty())
                .build();
            assert!(harness.focus_manager().dispatch_key_event(&event));
            harness.tick();
            assert_visible(&harness);
        }

        let select = KeyEventBuilder::new(Code::ArrowLeft)
            .with_key(Key::Named(NamedKey::ArrowLeft))
            .with_state(KeyState::Down)
            .with_modifiers(Modifiers::SHIFT)
            .build();
        assert!(harness.focus_manager().dispatch_key_event(&select));
        harness.tick();
        let selected = read(&field, |session| {
            session
                .rect_for_range(session.selection().range())
                .expect("selected geometry")
        });
        assert!(
            !selected.clipped,
            "the selected adjacent grapheme uses viewport coordinates"
        );
        let end = KeyEventBuilder::new(Code::End)
            .with_key(Key::Named(NamedKey::End))
            .with_state(KeyState::Down)
            .build();
        assert!(harness.focus_manager().dispatch_key_event(&end));
        harness.tick();

        // A visible suffix boundary is not the same x as its full-content
        // position. Tapping the caret must retain the byte insertion point.
        let rect = assert_visible(&harness);
        let before = controller.caret_byte_offset();
        harness.dispatch_pointer_down(rect.origin.x, rect.origin.y + rect.size.height / 2.0);
        harness.dispatch_pointer_up(rect.origin.x, rect.origin.y + rect.size.height / 2.0);
        assert_eq!(
            controller.caret_byte_offset(),
            before,
            "pointer inverse must include horizontal reveal"
        );
        assert!(
            harness
                .focus_manager()
                .dispatch_key_event(&super::character_key_event('!'))
        );
        assert_eq!(
            controller.text(),
            format!("{text}!"),
            "typing follows the tapped visible boundary"
        );
        harness.tick();
        assert_visible(&harness);

        harness.swap_root(SizedBox::new(30.0, 30.0).child(
            EditableText::new(controller.clone(), Rc::clone(&focus)).obscure_text(obscured),
        ));
        let units = flui_platform_api::text_store::utf16::utf16_len(&controller.text());
        let rect = read(&field, move |session| {
            session
                .rect_for_range(Utf16Range::collapsed(units))
                .expect("resized caret")
                .bounds
        });
        assert!(
            rect.origin.x >= -0.001 && rect.origin.x + rect.size.width <= 30.001,
            "resize reveals the same active caret: {rect:?}"
        );
        harness.dispatch_ime(&flui_platform_api::ImeEvent::Preedit {
            text: "xy".to_owned(),
            cursor: Some((0, 2)),
        });
        harness.tick();
        let composition = read(&field, |session| {
            session
                .rect_for_range(
                    session
                        .composition()
                        .expect("preedit established composition")
                        .range,
                )
                .expect("composition geometry")
        });
        assert!(
            !composition.clipped,
            "short preedit is revealed in the resized viewport"
        );
        assert!(
            composition.bounds.origin.x >= -0.001
                && composition.bounds.origin.x + composition.bounds.size.width <= 30.001,
            "composition geometry shares the text displacement: {composition:?}"
        );
    }

    pub(crate) fn editable_paint_places_long_text_under_the_viewport_clip() {
        use flui_painting::DrawOp;
        use flui_painting::paint::Clip;
        use flui_rendering::layer::Layer;
        use flui_widgets::SizedBox;
        let controller = TextEditingController::new();
        let focus = FocusNode::new();
        let mut laid = crate::common::lay_out(
            SizedBox::new(60.0, 30.0)
                .child(EditableText::new(controller.clone(), Rc::clone(&focus))),
            crate::common::tight(60.0, 30.0),
        );
        focus.request_focus();
        for ch in "abcdefghijklmnopqrstuvwxyz".chars() {
            laid.focus_manager()
                .dispatch_key_event(&super::character_key_event(ch));
        }
        laid.tick();
        let tree = laid.layer_tree().expect("typing painted a frame");
        let mut text_pictures = 0;
        for (id, node) in tree.iter() {
            let Layer::Picture(picture) = node.layer() else {
                continue;
            };
            if !picture
                .picture()
                .commands()
                .iter()
                .any(|command| matches!(command.op, DrawOp::Paragraph { .. }))
            {
                continue;
            }
            text_pictures += 1;
            let mut parent = tree.parent(id);
            let mut clipped = false;
            while let Some(id) = parent {
                if let Some(Layer::ClipRect(clip)) = tree.get_layer(id) {
                    clipped |=
                        clip.clip_behavior() == Clip::HardEdge && clip.clip_rect().width() == 60.0;
                }
                parent = tree.parent(id);
            }
            assert!(
                clipped,
                "the text's actual picture must descend from its viewport clip"
            );
        }
        assert!(text_pictures > 0, "input produced painted text");
    }
}

/// Event context (ADR-0086): `on_changed` and `on_submitted` run inside a
/// write the field opens from the writer source it acquired in
/// `init_state`, whichever path made the edit.
pub(crate) mod event_cx {
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

    pub(crate) fn typing_writes_through_on_changed_and_rebuilds_its_reader() {
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

    pub(crate) fn a_refused_write_in_on_changed_is_reported_not_panicked() {
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

/// Inserting before an existing mark joins a cluster; Backspace removes it whole.
pub(crate) fn insertion_keeps_the_caret_after_the_joined_combining_cluster() {
    let controller = TextEditingController::with_text("\u{301}");
    controller.set_caret_byte_offset(0);
    controller.insert_str("e");
    assert_eq!(controller.text(), "e\u{301}");
    assert_eq!(controller.caret_byte_offset(), controller.text().len());
    controller.backspace();
    assert_eq!(controller.text(), "");
    controller.insert_str("ok");
    assert_eq!(controller.caret_byte_offset(), 2);
}

pub(crate) fn deleting_a_separator_keeps_the_caret_after_the_joined_flag() {
    let controller = TextEditingController::with_text("🇦 🇧");
    controller.set_selection(4, 5);
    controller.insert_str("");
    assert_eq!(controller.text(), "🇦🇧");
    assert_eq!(controller.caret_byte_offset(), 8);
    controller.backspace();
    assert_eq!(controller.text(), "");
    controller.insert_str("x");
    assert_eq!(controller.caret_byte_offset(), 1);
}

fn selection_contact(id: u64) -> flui_interaction::PointerId {
    flui_interaction::PointerId::new(id).expect("nonzero fixture contact")
}

fn selection_field() -> (crate::common::LaidOut, TextEditingController, Rc<FocusNode>) {
    use flui_foundation::geometry::Size;
    use flui_rendering::constraints::BoxConstraints;
    let controller = TextEditingController::with_text("hello world");
    let focus = FocusNode::with_debug_label("persistent selection contact");
    let tree = crate::common::lay_out(
        EditableText::new(controller.clone(), Rc::clone(&focus)),
        BoxConstraints::tight(Size::new(500.0, 40.0)),
    );
    (tree, controller, focus)
}

pub(crate) fn selection_drag_survives_a_same_controller_rebuild() {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerType, make_cancel_event_for_id, make_down_event_for_id, make_move_event_for_id,
    };
    let (mut tree, controller, focus) = selection_field();
    let contact = selection_contact(41);
    tree.dispatch_pointer_event(&make_down_event_for_id(
        contact,
        Offset::new(1.0, 5.0),
        PointerType::Touch,
    ));
    let anchor = controller.caret_byte_offset();
    assert_eq!(anchor, 0);
    tree.pump_widget(EditableText::new(controller.clone(), Rc::clone(&focus)).caret_height(19.0));
    tree.dispatch_pointer_event(&make_move_event_for_id(
        contact,
        Offset::new(400.0, 5.0),
        PointerType::Touch,
    ));
    assert_eq!(controller.selection().start, anchor);
    assert!(controller.selection().end > anchor);
    tree.dispatch_pointer_event(&make_cancel_event_for_id(contact, PointerType::Touch));
    let next = selection_contact(42);
    tree.dispatch_pointer_event(&make_down_event_for_id(
        next,
        Offset::new(400.0, 5.0),
        PointerType::Touch,
    ));
    let next_anchor = controller.caret_byte_offset();
    tree.dispatch_pointer_event(&make_move_event_for_id(
        next,
        Offset::new(1.0, 5.0),
        PointerType::Touch,
    ));
    assert!(controller.caret_byte_offset() < next_anchor);
}

fn foreign_selection_terminal(cancel: bool) {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerType, make_cancel_event_for_id, make_down_event_for_id, make_move_event_for_id,
        make_up_event_for_id,
    };
    let (tree, controller, _focus) = selection_field();
    let own = selection_contact(51);
    let foreign = selection_contact(52);
    tree.dispatch_pointer_event(&make_down_event_for_id(
        own,
        Offset::new(1.0, 5.0),
        PointerType::Touch,
    ));
    let anchor = controller.caret_byte_offset();
    tree.dispatch_pointer_event(&make_down_event_for_id(
        foreign,
        Offset::new(150.0, 5.0),
        PointerType::Touch,
    ));
    assert_eq!(
        controller.caret_byte_offset(),
        anchor,
        "first contact owns selection"
    );
    tree.dispatch_pointer_event(&make_move_event_for_id(
        foreign,
        Offset::new(400.0, 5.0),
        PointerType::Touch,
    ));
    assert_eq!(
        controller.caret_byte_offset(),
        anchor,
        "foreign move cannot select"
    );
    let terminal = if cancel {
        make_cancel_event_for_id(foreign, PointerType::Touch)
    } else {
        make_up_event_for_id(foreign, Offset::new(400.0, 5.0), PointerType::Touch)
    };
    tree.dispatch_pointer_event(&terminal);
    tree.dispatch_pointer_event(&make_move_event_for_id(
        own,
        Offset::new(400.0, 5.0),
        PointerType::Touch,
    ));
    assert_eq!(controller.selection().start, anchor);
    assert!(
        controller.selection().end > anchor,
        "foreign terminal preserves own drag"
    );
    tree.dispatch_pointer_event(&make_cancel_event_for_id(own, PointerType::Touch));
    let next = selection_contact(53);
    tree.dispatch_pointer_event(&make_down_event_for_id(
        next,
        Offset::new(400.0, 5.0),
        PointerType::Touch,
    ));
    let next_anchor = controller.caret_byte_offset();
    tree.dispatch_pointer_event(&make_move_event_for_id(
        next,
        Offset::new(1.0, 5.0),
        PointerType::Touch,
    ));
    assert!(controller.caret_byte_offset() < next_anchor);
}

pub(crate) fn foreign_release_preserves_the_selection_contact() {
    foreign_selection_terminal(false);
}
pub(crate) fn foreign_cancel_preserves_the_selection_contact() {
    foreign_selection_terminal(true);
}

fn selection_retarget(replace: bool) {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerType, make_cancel_event_for_id, make_down_event_for_id, make_move_event_for_id,
    };
    let (mut tree, old, focus) = selection_field();
    let own = selection_contact(61);
    tree.dispatch_pointer_event(&make_down_event_for_id(
        own,
        Offset::new(1.0, 5.0),
        PointerType::Touch,
    ));
    let current = if replace {
        TextEditingController::with_text("replacement")
    } else {
        old
    };
    tree.pump_widget(EditableText::new(current.clone(), Rc::clone(&focus)).enabled(replace));
    if !replace {
        tree.pump_widget(EditableText::new(current.clone(), Rc::clone(&focus)));
    }
    let before = current.caret_byte_offset();
    tree.dispatch_pointer_event(&make_move_event_for_id(
        own,
        Offset::new(if replace { 1.0 } else { 400.0 }, 5.0),
        PointerType::Touch,
    ));
    assert_eq!(
        current.caret_byte_offset(),
        before,
        "retired contact cannot edit the current document"
    );
    tree.dispatch_pointer_event(&make_cancel_event_for_id(own, PointerType::Touch));
    let next = selection_contact(62);
    tree.dispatch_pointer_event(&make_down_event_for_id(
        next,
        Offset::new(400.0, 5.0),
        PointerType::Touch,
    ));
    let anchor = current.caret_byte_offset();
    tree.dispatch_pointer_event(&make_move_event_for_id(
        next,
        Offset::new(1.0, 5.0),
        PointerType::Touch,
    ));
    assert!(
        current.caret_byte_offset() < anchor,
        "new contact edits after retirement"
    );
}

pub(crate) fn disabling_the_field_retires_its_selection_contact() {
    selection_retarget(false);
}
pub(crate) fn replacing_the_controller_retires_the_old_selection_contact() {
    selection_retarget(true);
}
