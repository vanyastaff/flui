//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/editable_text_test.dart`
//! (tag `3.44.0`) — Flutter has no dedicated `text_editing_controller_test.dart`
//! at this tag (`git ls-tree -r 3.44.0 -- packages/flutter/test/widgets/` has
//! no such file); `TextEditingController`'s composing-region contract is
//! exercised only indirectly there, through `EditableTextState
//! .updateEditingValue`/`TextEditingValue`/`TextSelection` — Flutter's model
//! is "one immutable value with a selection AND a composing range", not
//! `flui_widgets::TextEditingController`'s "buffer + collapsed caret +
//! optional composing state" split (see that type's own module doc). This
//! file ports the handful of oracle assertions that translate onto FLUI's own
//! public controller API without a widget mount, and states plainly where the
//! translation is a **divergence** rather than a port — a "Rust-native
//! structure, Flutter-loyal behavior" case sometimes has no single Flutter
//! behavior to be loyal to, because the input models differ at the root
//! (Flutter's `TextInputConnection` value-diffing vs FLUI's winit-shaped
//! `ImeEvent` preedit/commit split, ADR-0030).
//!
//! `flui_widgets::text::controller`'s own `#[cfg(test)] mod tests` already
//! carries an extensive self-authored unit suite (buffer ops, caret
//! navigation, multi-byte correctness, composing lifecycle, mutator-clears
//! contract, listener notification) with its own red-check evidence in each
//! test's doc comment. This file does not duplicate that coverage; where a
//! genuine Flutter-oracle correspondence exists for one of those tests, the
//! citation was added to the existing test instead (see *Ported via citation*
//! below), matching this crate's `focus_test.rs` precedent. What lands here
//! as new test code is: (a) the same clamp-not-panic contract exercised
//! through the crate's **public** API from outside the crate (proving the
//! external contract, not just the internal implementation), and (b) two
//! divergences worth pinning explicitly because a future reader porting more
//! of `editable_text_test.dart` will otherwise re-discover them the hard way.
//!
//! ## Ported via citation (existing test, no new code)
//! - `'Asserts if composing text is not valid'` (`test`, not `testWidgets`) —
//!   Flutter throws constructing/assigning a `TextEditingValue` whose
//!   `composing` range has `start > end` (with `start`/`end` both `>= 0`) or
//!   `end` past `text.length`. FLUI's `set_composing_text` takes text +
//!   cursor directly (no standalone `TextEditingValue` to validate) and never
//!   asserts — malformed input clamps to the nearest valid boundary
//!   (`docs/PANIC-POLICY.md`: untrusted platform input must not panic). Cited
//!   at `TextEditingController::malformed_cursor_offset_past_the_preedit_end_clamps_without_panicking`,
//!   `::malformed_cursor_offset_mid_multibyte_char_clamps_forward_without_panicking`,
//!   and `::a_stale_composing_range_that_bypasses_the_mutator_guard_still_cannot_panic_commit`
//!   (`src/text/controller.rs`).
//! - `'Preserves composing range if cursor moves within that range'`,
//!   `'Clears composing range if cursor moves outside that range'`, and its
//!   `'case two'` variant — Flutter's `EditableTextState` clears the
//!   composing range the instant `controller.selection` moves outside it (a
//!   `didChangeText` into a fresh clean `TextEditingValue`, no explicit code
//!   path — arises from Flutter's "the composing range is a slice of the
//!   CURRENT selection-bearing value" representation). FLUI's caret is not a
//!   slice of a value the composing range is validated against on every
//!   move — direct caret navigation
//!   ([`TextEditingController::move_caret_left`] and its three siblings)
//!   explicitly does **not** touch the composing range, deliberately, so the
//!   user can glance the caret elsewhere mid-composition without losing the
//!   in-progress candidate text. See this file's
//!   [`direct_caret_navigation_leaves_the_composing_range_untouched_unlike_flutter`]
//!   for the divergence pinned from the external crate, and
//!   `TextEditingController::caret_navigation_restores_the_caret_while_composing`
//!   and `::clear_composing_leaves_a_caret_before_the_region_untouched`
//!   (`src/text/controller.rs`) for the pre-existing internally red-checked
//!   coverage of the same contract.
//!
//! ## Ported (new test code here)
//! - [`malformed_composing_cursor_clamps_through_the_public_api_instead_of_asserting`]
//!   — the clamp-not-assert contract above, driven through
//!   `flui_widgets::TextEditingController`'s public surface only (this file
//!   cannot reach the crate-private `ControllerInner` the internal tests use
//!   to fabricate a stale range) — proves the EXTERNAL contract survives
//!   malformed input, not just the internal implementation.
//! - [`direct_caret_navigation_leaves_the_composing_range_untouched_unlike_flutter`]
//!   — the composing-range-preservation divergence from the three oracle
//!   tests above, pinned from outside the crate.
//! - [`direct_caret_navigation_revokes_the_ime_hidden_caret_flag_but_leaves_composition_running`]
//!   — **no Flutter oracle**: Flutter's `TextInputConnection` protocol has no
//!   "hide the caret" signal distinct from the composing range itself — every
//!   composing update carries its own selection, so there is nothing
//!   analogous to winit's `Preedit { cursor: None }` to have a test about.
//!   `caret_hidden_by_ime` (ADR-0032/0033) is a leapfrog addition at an edge
//!   Flutter has no strong contract for (`AGENTS.md` rule 2) — included here
//!   as core controller-model coverage the task's "caret-navigation revoke"
//!   scope calls for, labeled honestly as oracle-less rather than forcing a
//!   fake citation onto it.
//! - [`backspace_after_a_zwj_family_emoji_breaks_the_grapheme_cluster_unlike_flutter`]
//!   — Flutter's `'Can access characters on editing string'` asserts that an
//!   inserted extended grapheme cluster (`'👨‍👩‍👦'`, a `characters.length`
//!   of 1 despite spanning 5 UTF-16 code units) round-trips through
//!   `onChanged` with the RIGHT `characters` count — Flutter's delete-by-
//!   character operations (`TextEditingController`'s associated
//!   `RenderEditable` deletion path) are grapheme-cluster-aware via the
//!   `characters` package. FLUI's `TextEditingController::backspace` walks
//!   back exactly one Rust `char` (one Unicode scalar value), which is
//!   correct for a plain multi-byte character (see
//!   `backspace_removes_full_multibyte_char`, `src/text/controller.rs`) but
//!   is NOT grapheme-cluster-aware: backspacing after inserting a
//!   Zero-Width-Joiner emoji sequence removes only the trailing scalar,
//!   leaving a dangling joiner. A genuine, documented divergence — not a
//!   silently dropped feature, since FLUI's `[`DEFERRED (v1)`]` list already
//!   states multi-byte correctness as scalar-level, not grapheme-level.
//!
//! ## Out of scope, with reasons
//! - `'will not cause crash while the TextEditingValue is composing'`,
//!   `'handles composing text correctly, continued'`, `'enforced composing
//!   truncated'`, `'default truncate behaviors with different platforms'`,
//!   `"composing range removed if it's overflowed the truncated value's
//!   length"`, `'composing range removed with different platforms'`,
//!   `"composing range handled correctly when it's overflowed"`, `'typing in
//!   the middle with different platforms.'` — all exercise
//!   `LengthLimitingTextInputFormatter`; FLUI's `EditableText` has no input-
//!   formatter pipeline (a named `DEFERRED (v1)` item on both
//!   `EditableText` and `TextEditingController`).
//! - `'Composing region can truncate grapheme'` — same input-formatter
//!   pipeline dependency as above.
//! - Every `Floating cursor *` test — floating-cursor drag gestures are a
//!   touch-selection-handle feature; FLUI has no selection/drag-handle
//!   support (`DEFERRED (v1)`).
//! - `'Selection is updated when the field has focus and the new selection is
//!   invalid'`, and every other test whose subject is `TextSelection`
//!   (non-collapsed, base/extent) — FLUI's controller tracks only a
//!   collapsed caret; there is no selection model to port a selection-range
//!   test onto.

use flui_widgets::TextEditingController;

/// Oracle: `'Asserts if composing text is not valid'` (`test`,
/// `editable_text_test.dart`, tag `3.44.0`) — see this file's module doc for
/// the divergence (Flutter asserts/throws; FLUI clamps).
///
/// Driven entirely through `flui_widgets::TextEditingController`'s public
/// surface (this external test crate cannot reach the crate-private
/// `ControllerInner` the internal `a_stale_composing_range_that_bypasses_the
/// _mutator_guard_still_cannot_panic_commit` test fabricates a stale range
/// through) — proving the same clamp-not-panic contract holds from outside
/// the crate, not just against the internal implementation.
#[test]
fn malformed_composing_cursor_clamps_through_the_public_api_instead_of_asserting() {
    let controller = TextEditingController::new();

    // A cursor end index far past the preedit text's own length — the exact
    // "start/end out of bounds" shape Flutter's assertion rejects outright.
    controller.set_composing_text("ni", Some((0, 100)));

    assert_eq!(
        controller.composing_range(),
        Some(0..2),
        "an out-of-range cursor clamps to the preedit's own length instead of \
         panicking or asserting"
    );
    assert_eq!(controller.caret_byte_offset(), 2);
    assert!(controller.is_composing());
}

/// Oracle: `'Preserves composing range if cursor moves within that range'`,
/// `'Clears composing range if cursor moves outside that range'`, and its
/// `'case two'` variant (`editable_text_test.dart`, tag `3.44.0`) — see this file's
/// module doc for the divergence this pins: Flutter clears the composing
/// range the instant the selection moves outside it; FLUI's direct caret
/// navigation never touches the composing range at all, regardless of where
/// the caret lands relative to it.
#[test]
fn direct_caret_navigation_leaves_the_composing_range_untouched_unlike_flutter() {
    let controller = TextEditingController::with_text("Hello ");
    controller.set_composing_text("wor", Some((3, 3)));
    let composing_before = controller.composing_range();
    assert_eq!(
        composing_before,
        Some(6..9),
        "precondition: composing region set"
    );

    // Home moves the caret to byte 0 — strictly BEFORE the composing region,
    // the exact "moves outside that range" shape Flutter's oracle clears on.
    controller.move_caret_home();
    assert_eq!(
        controller.composing_range(),
        composing_before,
        "FLUI's direct caret navigation must not clear the composing range, \
         even when the caret lands outside it — divergent from Flutter, \
         which clears on exactly this move"
    );
    assert!(
        controller.is_composing(),
        "the composition itself keeps running after the caret moves away"
    );

    // End moves the caret past the composing region too — the same
    // "moves outside" shape from the other direction.
    controller.move_caret_end();
    assert_eq!(controller.composing_range(), composing_before);
    assert!(controller.is_composing());
}

/// **No Flutter oracle** — see this file's module doc. Pinned as core
/// controller-model coverage of the caret-hidden revoke contract
/// (ADR-0032/0033), a leapfrog addition at an edge Flutter has no strong
/// contract for.
///
/// Mirrors the pre-existing, internally red-checked
/// `TextEditingController::caret_navigation_restores_the_caret_while_composing`
/// (`src/text/controller.rs`; that test's own doc records the verified
/// red-check: removing the `clear_caret_hidden` call from `move_caret_home`
/// leaves `caret_hidden_by_ime()` `true` after this exact sequence) — proven
/// again here from outside the crate.
#[test]
fn direct_caret_navigation_revokes_the_ime_hidden_caret_flag_but_leaves_composition_running() {
    let controller = TextEditingController::with_text("abc");
    controller.set_composing_text("def", None); // cursor: None hides the caret.
    assert!(
        controller.caret_hidden_by_ime(),
        "precondition: caret hidden by IME"
    );

    controller.move_caret_left();

    assert!(
        !controller.caret_hidden_by_ime(),
        "direct caret navigation must revoke the IME hidden-caret flag"
    );
    assert!(
        controller.is_composing(),
        "revoking the hidden-caret flag must not end the composition itself"
    );
}

/// Oracle: `'Can access characters on editing string'`
/// (`editable_text_test.dart`, tag `3.44.0`) — adapted into a documented
/// divergence; see this file's module doc for why Flutter's grapheme-cluster-
/// aware deletion has no FLUI counterpart.
#[test]
fn backspace_after_a_zwj_family_emoji_breaks_the_grapheme_cluster_unlike_flutter() {
    // MAN + ZWJ + WOMAN + ZWJ + BOY — one extended grapheme cluster (Flutter's
    // `characters.length == 1`), five Unicode scalar values, 18 UTF-8 bytes
    // (4 + 3 + 4 + 3 + 4).
    const FAMILY_EMOJI: &str = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F466}";
    const FAMILY_EMOJI_MINUS_BOY: &str = "\u{1F468}\u{200D}\u{1F469}\u{200D}";

    let controller = TextEditingController::new();
    controller.insert_str(FAMILY_EMOJI);
    assert_eq!(controller.text(), FAMILY_EMOJI);
    assert_eq!(controller.caret_byte_offset(), 18);

    controller.backspace();

    assert_eq!(
        controller.text(),
        FAMILY_EMOJI_MINUS_BOY,
        "FLUI's backspace removes exactly one Unicode scalar value (the \
         trailing BOY glyph), leaving a dangling Zero-Width-Joiner — a \
         broken grapheme cluster. Flutter's grapheme-aware deletion would \
         remove the whole 5-scalar cluster in one Backspace; FLUI has no \
         grapheme-cluster segmentation in v1 (see the crate's DEFERRED list)"
    );
    assert_eq!(controller.caret_byte_offset(), 14);
    assert!(
        controller.text().ends_with('\u{200D}'),
        "the buffer is left ending in a dangling joiner, not a clean glyph boundary"
    );
}
