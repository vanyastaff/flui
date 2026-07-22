//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/editable_text_test.dart`
//! (tag `3.44.0`, 324 `testWidgets`/`test` cases by
//! `grep -cE '^\s*(testWidgets|test)\(' `). `src/text/editable_text.rs`'s own
//! `#[cfg(test)] mod tests` already carries an extensive self-authored suite
//! (focus attach/detach, IME client attach/detach, suppression contract,
//! composing-underline geometry, the ADR-0032 cursor-area loop) with its own
//! documented red-check evidence. This file's job matches this crate's
//! `focus_test.rs` precedent: anchor a small set of cases to a **named
//! upstream test** and state the ported/adapted/divergent relationship
//! explicitly, citing existing well-covered internal tests rather than
//! duplicating a harness that already exercises the exact behavior.
//!
//! `EditableText`'s own module doc already states most of the portable-core
//! constraints this file works within: v1 has no selection (only a collapsed
//! caret), no clipboard, no multiline, no `obscureText`, no input formatters,
//! and no controller-swap re-registration — every Flutter oracle test that
//! depends on one of those is out of scope, listed below with its reason.
//!
//! ## Ported (new test code here)
//! - [`composing_underline_geometry_appears_while_composing_and_disappears_after_unfocus`]
//!   — oracle: `'Composing text is underlined and underline is cleared when
//!   losing focus'`. Flutter's assertion reads the actual `TextSpan` tree
//!   (three children, the middle one carrying `TextDecoration.underline`);
//!   FLUI has no `TextStyle.decoration` to merge (declared divergence,
//!   ADR-0033 / `RenderEditable`'s module doc: a flat approximated underline
//!   rect, not real font metrics) — ported as **geometry-relative**:
//!   `RenderEditable::rect_for_composing_range()` is `Some` while composing
//!   and focused, and `None` once the field blurs, exercised through THIS
//!   crate's external parity harness (`crate::common::lay_out`/`tick`), a
//!   different mounting path than the internal `mount_with_ime` harness the
//!   pre-existing `unfocus_mid_composition_stops_passing_the_composing_range`
//!   test uses (same underlying `HeadlessBinding`, same `if focused { ... }
//!   else { None }` gate in `build_field_view` — see that test's own
//!   red-check for the mutation this shares).
//!
//! ## Ported via citation (existing test, no new code here)
//! - `'Composing text is underlined and underline is cleared when losing
//!   focus'` (the caret-visibility half) — already exercised by
//!   `preedit_cursor_none_while_focused_hides_the_caret_and_starts_the_underline`,
//!   `preedit_cursor_some_while_focused_keeps_the_caret_visible`,
//!   `commit_removes_the_underline_and_restores_the_caret`
//!   (`src/text/editable_text.rs`).
//! - `'connection is closed when TextInputClient.onConnectionClosed message
//!   received'` — **adapted, not a direct port**: Flutter's `connectionClosed`
//!   only ends the input session (`wantKeepAlive` goes `false`; the buffer
//!   itself is untouched, per the oracle's own comment "This makes sure
//!   hide/clearClient methods are not called"). FLUI's `ImeEvent::Disabled`
//!   additionally **strips the in-progress composing slice** from the
//!   buffer — winit's own semantics, a documented divergence already stated
//!   in `RenderEditable`'s and `TextEditingController::clear_composing`'s doc
//!   comments. Exercised by
//!   `disabled_removes_the_underline_and_restores_the_caret` and
//!   `disabled_mid_preedit_strips_the_composing_slice_through_the_attached_client`
//!   (`src/text/editable_text.rs`).
//! - `'Does not accept updates when read-only'` — **adapted, not a direct
//!   port**: Flutter's `readOnly` blocks platform text updates while the
//!   field KEEPS focus (the oracle interacts with the field, then asserts
//!   only that `hasAnyClients` stays consistent, never that focus was
//!   refused). `EditableText::enabled` (a named hoist of `TextField.enabled`
//!   one layer down — see that field's own doc comment) is a strictly wider
//!   gate: disabling withholds focus ACQUISITION entirely, and disabling a
//!   currently-focused field releases its focus outright — see this file's
//!   module doc's *Not ported* note for why no new external-harness test
//!   duplicates this: `disabled_field_does_not_publish_its_focus_node`,
//!   `enabled_field_publishes_its_focus_node`,
//!   `disabling_a_focused_field_unfocuses_it_and_withdraws_the_node`,
//!   `re_enabling_a_disabled_field_republishes_its_focus_node`, and
//!   `disabled_key_handler_ignores_input_even_when_invoked_directly`
//!   (`src/text/editable_text.rs`) already exercise this contrast through an
//!   equivalent `HeadlessBinding`-backed harness (`mount`/`swap_root`, the
//!   crate-internal twin of this file's `lay_out`/`pump_widget`) — a second
//!   near-identical mount through this file's harness would prove the same
//!   fact through the same machinery for no new evidence.
//!
//! ## Not ported, with reasons
//! - Every `Length formatter` / `LengthLimitingTextInputFormatter` case,
//!   `'Composing region can truncate grapheme'` — no input-formatter
//!   pipeline (`DEFERRED (v1)`).
//! - Every `Floating cursor *` case, the whole `text selection toolbar`
//!   group, `'bringIntoView brings the caret into view...'` — drag-selection,
//!   selection handles, and the floating cursor are all selection-model
//!   features FLUI does not implement (collapsed caret only, `DEFERRED
//!   (v1)`).
//! - `'does not refocus when it is unmounted'`, `'does not refocus when it is
//!   hidden by a new route'`, `'does not refocus when scrolled away in a
//!   ListView'`, `'closed connection reopened when user focused'` and its
//!   variant for another field, `'can re-acquire focus when the platform
//!   sends onFocusReceived'`, `'can re-acquire focus in Offstage'` — all exercise
//!   Flutter's two-phase `TextInputClient.onFocusReceived`/`connectionClosed`
//!   platform-reacquisition protocol, which FLUI's single-shot IME attach/
//!   detach (ADR-0030) has no equivalent phase for.
//! - Every `keyboard is requested`/`Keyboard is configured for "..." action`
//!   case, `'insertContent does not throw...'`, autofill-hint inference,
//!   `'text styling info is sent on show keyboard'` (+ bold override),
//!   `'location of widget is sent on show keyboard'` — all assert the
//!   `TextInputConfiguration`/`setEditingState` payload FLUI's headless
//!   `TextInputHandle` capability does not model at this layer (no
//!   `keyboardType`/`textInputAction`/autofill fields on `EditableText`).
//! - `'Can access characters on editing string'`, `'RTL arabic correct caret
//!   placement after trailing whitespace'`, every accessibility (`a11y`)
//!   cursor-movement case, every `Semantics` case — grapheme-cluster
//!   iteration is ported as a controller-level DIVERGENCE instead (see
//!   `text_editing_controller_test.rs`'s
//!   `backspace_after_a_zwj_family_emoji_breaks_the_grapheme_cluster_unlike_flutter`);
//!   bidi/RTL shaping and accessibility integration are not implemented.
//! - `'Cursor color with an opacity is respected'`, cursor-blink-animation
//!   cases — FLUI's caret has no blink animation or opacity channel in v1
//!   (a flat `show_caret: bool`).
//! - `'delete doesn't cause crash when selection is -1,-1'` — Flutter's
//!   `(-1, -1)` unsettable-selection edge case has no FLUI representation
//!   (no selection model at all); the underlying "never panic on an
//!   at-the-boundary delete" guarantee this pins is already covered by
//!   `TextEditingController::backspace_at_start_is_noop` /
//!   `::delete_forward_at_end_is_noop`.
//! - Every remaining case (autofill, spellcheck, mouse-cursor-on-hover,
//!   scribble, `default_text_editing_shortcuts`, obscured-text metrics,
//!   Ahem-exact glyph-position cases) — out of scope per this port's task
//!   description: obscureText/multiline/gestures-on-text/spellcheck/autofill/
//!   keyboardType/exact-Ahem-metrics are all named deferrals or the
//!   established geometry-relative text-family precedent (cosmic-text has no
//!   Ahem).

use flui_interaction::routing::FocusManager;
use flui_objects::RenderEditable;
use flui_widgets::{EditableText, TextEditingController};
use parking_lot::Mutex;

use crate::common::{LaidOut, lay_out, loose};

/// Conservatively serializes this file's focus fixtures on top of
/// `FocusManager::global()`'s owner-thread singleton — the same convention
/// `focus_test.rs` and `src/test_harness.rs`'s `FOCUS_TEST_LOCK` use.
static FOCUS_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Runs `f` against the mounted field's single `RenderEditable`, found by
/// downcasting the one render object `EditableText` mounts — the external-
/// harness twin of `src/text/editable_text.rs`'s crate-internal
/// `with_render_editable` test helper.
fn with_render_editable<T>(laid: &LaidOut, f: impl FnOnce(&RenderEditable) -> T) -> Option<T> {
    let owner = laid.pipeline_owner();
    let owner = owner.read();
    for (_, node) in owner.render_tree().iter() {
        let editable = node
            .as_box()
            .and_then(|entry| entry.render_object().downcast_ref::<RenderEditable>()); // PORT-CHECK-OK-DOWNCAST: test-only reach to the one concrete render object type this widget mounts, through the storage layer's `&dyn RenderObject<BoxProtocol>` erasure — see docs/PORT.md FR-033/widgets.
        if let Some(editable) = editable {
            return Some(f(editable));
        }
    }
    None
}

/// Oracle: `'Composing text is underlined and underline is cleared when
/// losing focus'` (`editable_text_test.dart`, tag `3.44.0`) — ported
/// geometry-relative (see this file's module doc for why: no
/// `TextStyle.decoration` to source real underline metrics from).
#[test]
fn composing_underline_geometry_appears_while_composing_and_disappears_after_unfocus() {
    let _guard = FOCUS_TEST_LOCK.lock();
    FocusManager::global().unfocus();

    let controller = TextEditingController::new();
    let mut laid = lay_out(EditableText::new(controller.clone()), loose(200.0));
    let node_id = controller
        .focus_node_id()
        .expect("an enabled field publishes its node");

    FocusManager::global().request_focus(node_id);
    laid.tick();

    assert!(
        with_render_editable(&laid, RenderEditable::show_caret).unwrap_or(false),
        "precondition: the caret paints while focused with no composition"
    );
    assert!(
        with_render_editable(&laid, RenderEditable::rect_for_composing_range)
            .flatten()
            .is_none(),
        "precondition: no composing geometry before any composition starts"
    );

    controller.set_composing_text("ni", None);
    laid.tick();

    assert!(
        with_render_editable(&laid, RenderEditable::rect_for_composing_range)
            .flatten()
            .is_some(),
        "an active composing range while focused must produce composing \
         geometry — the oracle's underlined middle TextSpan"
    );

    FocusManager::global().unfocus();
    laid.tick();

    assert!(
        controller.is_composing(),
        "blurring alone must not end the composition — only a Commit/\
         Disabled/empty-Preedit event does (ADR-0030)"
    );
    assert!(
        with_render_editable(&laid, RenderEditable::rect_for_composing_range)
            .flatten()
            .is_none(),
        "an unfocused field must stop painting a stale composing underline — \
         the oracle's 'underline is cleared when losing focus'"
    );

    FocusManager::global().unfocus();
}
