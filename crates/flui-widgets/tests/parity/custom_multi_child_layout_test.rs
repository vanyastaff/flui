//! ## Test parity notes
//!
//! Flutter source:
//! - Widget: `packages/flutter/lib/src/widgets/basic.dart`
//!   `CustomMultiChildLayout` + `LayoutId` over `MultiChildLayoutDelegate`
//!   (`packages/flutter/lib/src/rendering/custom_layout.dart`).
//! - Render object: `packages/flutter/lib/src/rendering/custom_layout.dart`
//!   `RenderCustomMultiChildLayoutBox`.
//! - Tests: `packages/flutter/test/widgets/custom_multi_child_layout_test.dart`
//!   (tag `3.44.0`), all 12 cases (5 top-level `testWidgets` +
//!   `group('performLayout error control test')`'s 7).
//!
//! ## How a delegate contract violation actually surfaces in FLUI
//!
//! Read this before the case ledger — it explains why cases 6, 7, 8, 9, 10,
//! and 11 assert committed geometry rather than `#[should_panic]`, and why
//! case 12 needs a capturing `tracing` subscriber.
//!
//! The oracle's `group('performLayout error control test')` relies on
//! Flutter's synchronous, catchable `FlutterError`: `layoutChild`,
//! `positionChild`, and `_callPerformLayout` `assert()` their invariants and
//! throw, `tester.pumpWidget` lets the exception surface to
//! `FlutterError.onError`, and the test asserts on the exact message text.
//! The natural Rust transliteration is a `panic!` — and
//! `DelegateLayoutContext`/`RenderCustomMultiChildLayoutBox`
//! (`crates/flui-objects/src/layout/custom_multi_child_layout.rs`) do
//! exactly that, confirmed by reading the source (`index_for`, the duplicate
//! `layout_child` assert, and `finish`'s "must be laid out exactly once"
//! panic all still exist and still fire — verified below, not assumed).
//!
//! But that panic **never reaches a test calling `harness::pump_widget`**.
//! FLUI's render pipeline wraps every node's `perform_layout_raw` call in its
//! own `std::panic::catch_unwind`
//! (`crates/flui-rendering/src/pipeline/owner/subtree_arena.rs`,
//! `layout_subtree_borrowed_impl`), converts a caught panic into
//! `RenderError::Poisoned { render_object, phase }`
//! (`crates/flui-rendering/src/error.rs`), and
//! `crates/flui-rendering/src/pipeline/owner/layout.rs` /
//! `crates/flui-rendering/src/pipeline/owner/poison.rs` record that node as
//! **layout-poisoned**: its geometry is never committed, but the rest of the
//! frame — every sibling, every ancestor above it — still completes
//! normally. This was verified empirically (not inferred from reading
//! alone): a probe that pumps `ZeroAndOneIdLayoutDelegate` against a
//! single-child frame confirms `harness::pump_widget` returns without
//! panicking, while `flui_rendering::testing::inspect::box_geometry` for the
//! `RenderCustomMultiChildLayoutBox` node comes back `None`. A full
//! `RUST_BACKTRACE=1` capture of that same probe shows the original
//! `panic!("...tried to access a non-existent child id...")` firing exactly
//! where the source says it should, then being caught two frames later by
//! `catch_unwind` inside `layout_subtree_borrowed_impl` — so the panic is
//! real, the pipeline's recovery is real, and no test in this file needs to
//! assume either.
//!
//! The practical fallout for this port:
//! - **Cases 6, 7, 9, 10, 11** assert `box_geometry(subject) == None` for the
//!   `RenderCustomMultiChildLayoutBox` node — the pipeline's actual
//!   observable signal that it rejected the delegate's contract violation —
//!   rather than `#[should_panic]`, which would report "test did not panic"
//!   (verified: it was the first form these five cases took, and every one
//!   failed exactly that way before this rewrite).
//! - **Case 8** asserts the same signal on the *child's* geometry instead of
//!   the parent's, and for a subtler reason documented at its own test.
//! - **Case 12** needs the *text* of the caught panic to check whether it
//!   names one forgotten child or several, and that text is not part of
//!   `RenderError::Poisoned` at all (only `render_object` + `phase`) — it
//!   exists solely as a `tracing::error!(panic_msg = ..., ...)` diagnostic
//!   inside `subtree_arena.rs`. A capturing `tracing::Subscriber` (installed
//!   thread-locally for the one call via `tracing::subscriber::with_default`,
//!   never process-global) is therefore the only way to make that divergence
//!   falsifiable at all — confirmed by capturing the real log line first:
//!   `panic_msg="Each child of RenderCustomMultiChildLayoutBox must be laid
//!   out exactly once; missing id \"2\""`, with `"3"` nowhere in it.
//!
//! This is not itself a bug: a render object whose delegate misbehaves being
//! isolated (no committed geometry, no crash) while the rest of the frame
//! renders is a *more* resilient outcome than Flutter's — it just means the
//! *mechanism* this port's tests must observe is structurally different from
//! the oracle's, not that the underlying contract ("a misbehaving delegate
//! must not silently produce a valid layout") went unenforced.
//!
//! ## Case ledger
//!
//! 1. `'Control test for CustomMultiChildLayout'` — ported, real green:
//!    [`control_test_delegate_sees_the_incoming_constraints_and_the_laid_out_children`].
//! 2. `'Test MultiChildDelegate shouldRelayout method'` — ported as **three
//!    separate assertions**, the first two real green and the last an
//!    `#[ignore]`d divergence pin asserting the oracle:
//!    [`should_relayout_is_not_called_on_the_first_layout`],
//!    [`should_relayout_is_consulted_when_the_delegate_is_replaced`],
//!    [`a_false_should_relayout_prevents_relayout_pin`].
//! 3. `'Nested CustomMultiChildLayouts'` — ported, real green, but
//!    strengthened beyond the oracle: [`nested_layouts_reusing_the_same_delegate_instance_lay_out_independently`].
//!    The oracle's own body carries **no `expect()` calls at all** — its only
//!    claim is that pumping does not throw when the identical delegate
//!    instance backs both the outer and inner `CustomMultiChildLayout`. FLUI's
//!    `DelegateLayoutContext` (the object the delegate borrows during
//!    `perform_layout`) is a fresh per-call value owned by the render object's
//!    own layout call, not state stashed on the delegate itself (contrast
//!    Flutter's `_idToChild`/`_debugChildrenNeedingLayout`, fields
//!    save/restored around a `_callPerformLayout` call specifically so a
//!    reentrant delegate cannot corrupt itself) — so there is no state on the
//!    FLUI delegate for the nesting to corrupt in the first place, and an
//!    "it didn't throw" assertion would be vacuous here. This port instead
//!    pins the actual geometry every level resolves to, which both subsumes
//!    "did not panic" and is falsifiable on its own terms.
//! 4. `'Loose constraints'` — ported, real green:
//!    [`replacing_the_delegate_resizes_the_render_object_with_no_children`].
//! 5. `'Can use listener for relayout'` — **out of scope by missing feature,
//!    no pin**: the oracle's `MultiChildLayoutDelegate({Listenable? relayout})`
//!    lets a delegate re-layout without being replaced, by driving a
//!    `ValueNotifier` the render object listens to. FLUI's
//!    `MultiChildLayoutDelegate` has no `relayout` member and
//!    `RenderCustomMultiChildLayoutBox` subscribes to nothing, so there is no
//!    observable to assert on — a test here could only re-assert case 4
//!    through a different surface. Adding the listener is a delegate-API
//!    change with its own subscribe/unsubscribe lifecycle across
//!    `set_delegate`, not a test gap. (Same disposition, same rationale, as
//!    `custom_single_child_layout_test.rs` case 4 for the single-child
//!    sibling delegate.)
//! 6. `group('performLayout error control test')` → `'layoutChild on non
//!    existent child'` — ported, real green:
//!    [`layout_child_on_a_non_existent_id_leaves_the_subject_unlaidout`].
//! 7. `'layoutChild more than once'` — ported, real green:
//!    [`layout_child_called_twice_for_the_same_id_leaves_the_subject_unlaidout`].
//! 8. `'layoutChild on invalid size constraint'` — ported, real green (with a
//!    reported fragility, not a pin — see below):
//!    [`layout_child_with_an_inverted_size_constraint_leaves_the_child_unlaidout`].
//! 9. `'positionChild on non existent child'` — ported, real green:
//!    [`position_child_on_a_non_existent_id_leaves_the_subject_unlaidout`].
//! 10. `"_callPerformLayout on child that doesn't have id"` — ported, real
//!     green: [`a_child_without_a_layout_id_leaves_the_subject_unlaidout`].
//!     FLUI has no `ParentDataWidget`-authoring path a test needs to reach
//!     this from (the oracle builds a bespoke `LayoutWithMissingId extends
//!     ParentDataWidget<MultiChildLayoutParentData>` purely to construct a
//!     child with no id); simply omitting the `LayoutId` wrapper produces the
//!     identical parent-data state (`MultiChildLayoutParentData::default()`,
//!     `id: None`) far more directly.
//! 11. `'performLayout did not layout a child'` — ported, real green:
//!     [`a_child_never_laid_out_names_it_in_the_captured_log`].
//! 12. `'performLayout did not layout multiple child'` — **divergence pin**:
//!     [`multiple_children_never_laid_out_are_all_named_in_the_panic_pin`].
//!     See "Divergence found by this port" below.
//!
//! ## Divergence found by this port
//!
//! **Case 2's suppressed-relayout half (the pin at
//! [`a_false_should_relayout_prevents_relayout_pin`])** is the same gap
//! `custom_single_child_layout_test.rs` already documents and pins for the
//! single-child delegate:
//! `RenderCustomMultiChildLayoutBox::set_delegate` computes Flutter's exact
//! verdict (`type_changed || delegate.should_relayout(old)`) but
//! `CustomMultiChildLayout::update_render_object`
//! (`crates/flui-widgets/src/layout/custom_multi_child_layout.rs`) discards
//! the returned `bool`, and `RenderBehavior::on_update`
//! (`crates/flui-view/src/element/behavior.rs`) marks the subject
//! needs-layout unconditionally on every re-pump regardless of what any
//! delegate answered. Not re-explained in full here — see that file's module
//! doc for the two-part fix this needs.
//!
//! **Case 12 is a second, independent finding, specific to the multi-child
//! delegate:** `DelegateLayoutContext::finish`
//! (`crates/flui-objects/src/layout/custom_multi_child_layout.rs`) is
//!
//! ```ignore
//! fn finish(self) {
//!     if let Some(index) = self.laid_out.iter().position(|laid_out| !*laid_out) {
//!         panic!(
//!             "Each child of RenderCustomMultiChildLayoutBox must be laid out exactly once; missing id {:?}",
//!             self.slots.index_to_id[index]
//!         );
//!     }
//! }
//! ```
//!
//! `Iterator::position` stops at the **first** match, so when two or more
//! children are left unlaid the panic names only the first of them — the
//! oracle's own two "did not lay out a child" cases exist specifically to
//! distinguish this (case 11's oracle message says "the following **child**"
//! naming one id; case 12's says "the following **children**" and enumerates
//! all of them, `2:` and `3:` on their own lines via `DiagnosticsBlock`).
//! Captured directly from a real run (see `debug_probe_captured_log_text`,
//! since deleted after confirming the exact text — the diagnostic line was):
//!
//! ```text
//! panic_msg="Each child of RenderCustomMultiChildLayoutBox must be laid out
//! exactly once; missing id \"2\""
//! ```
//!
//! `"3"` never appears anywhere in it. Case 11 (exactly one forgotten child)
//! is therefore a faithful, real-green port; case 12 (two forgotten) is
//! pinned against the oracle's completeness contract, `#[ignore]`d rather
//! than asserted-as-is, because asserting today's truncated message would
//! lock in the information loss as correct and turn the eventual fix
//! (accumulate every unlaid index, not just the first) into a red
//! "regression". This is a diagnostic-completeness gap, not a soundness one
//! — the render object still refuses to commit geometry either way — which
//! is why it does not block the rest of this port.
//!
//! **A third finding, reported rather than fixed (case 8 is real green, not
//! a pin, but the "why" is worth flagging):** Flutter's `layoutChild` rejects
//! an inverted `BoxConstraints` (e.g. `min_width = 0 > max_width = -1`) via
//! `debugAssertIsValid` *before* the invalid constraints ever reach the
//! child. FLUI's `BoxConstraints`
//! (`crates/flui-rendering/src/constraints/box_constraints.rs`) has no
//! validity invariant anywhere — its own `round_for_cache` doc comment notes
//! a real `normalize()` (min ≥ 0, max ≥ min) as a `TODO` — and
//! `DelegateLayoutContext::layout_child` forwards whatever `BoxConstraints`
//! it is given straight through unchecked. What actually stops an inverted
//! constraint from silently producing a nonsensical committed geometry in
//! this port's test is `Pixels::clamp` (`crates/flui-geometry/src/units.rs`)
//! delegating to `f32::clamp`, whose standard-library contract panics when
//! `min > max` — confirmed empirically: a probe using
//! `InvalidConstraintsChildLayoutDelegate` panics at
//! `core/src/num/f32.rs:1565:9`, inside the child's own layout, not at any
//! named contract check. The render pipeline's recovery then poisons that
//! child exactly as it would any other panic, so the *outcome* happens to
//! match Flutter's (the child never commits a valid geometry) — but for an
//! entirely different, fragile reason: the rejection is incidental to one
//! particular child render object's internal arithmetic reaching a
//! `min > max` clamp, not a validated contract at the point of misuse. A
//! future `Pixels::clamp` reimplementation that avoids `f32::clamp`'s panic
//! (e.g. `self.0.max(min.0).min(max.0)`, which *silently accepts* an
//! inverted range instead of panicking) would remove this safety net with
//! zero test coverage at the `BoxConstraints` level catching the regression
//! — only this one, incidental, port-level assertion would, and only
//! because this specific child happens to route through `.clamp()`. Not
//! pinned, because there is no FLUI behavior to assert against that is
//! currently false — the gap is the *absence* of a deliberate contract, which
//! a pin (a currently-failing assertion of what should be true) cannot
//! express. Filed here for `rust-builder`/`qa-lead` triage.
//!
//! ## Notes (not pinned — no observable behavior difference)
//!
//! `DelegateLayoutContext::index_for` is the single panic site backing both
//! [`MultiChildLayoutContext::layout_child`] and
//! [`MultiChildLayoutContext::position_child`] for a non-existent id, so
//! FLUI's message text is identical for cases 6 and 9 ("tried to access a
//! non-existent child id"). The oracle uses two distinct messages ("tried to
//! lay out a non-existent child" vs. "tried to position out a non-existent
//! child"). Both FLUI operations still leave the subject unlaidout on an
//! invalid id — the pass/fail contract this port tests matches the oracle
//! exactly — so this is a wording difference in an internal diagnostic, not
//! a pinnable behavior divergence.

use std::any::Any;
use std::sync::{Arc, Mutex};

use flui_foundation::RenderId;
use flui_rendering::constraints::BoxConstraints;
use flui_rendering::delegates::{MultiChildLayoutContext, MultiChildLayoutDelegate};
use flui_types::geometry::px;
use flui_types::{Offset, Size};
use flui_view::{BoxedView, View, ViewExt};
use flui_widgets::{Center, CustomMultiChildLayout, LayoutId, SizedBox};

use crate::common::LaidOut;
use crate::harness;

// ============================================================================
// Fixtures
// ============================================================================

/// True when `id` has no committed box geometry — the pipeline's observable
/// signal that it caught a panic in that node's own `perform_layout` and
/// "layout-poisoned" it, rather than committing a size, while the rest of
/// the frame still completed. See "How a delegate contract violation
/// actually surfaces in FLUI" in the module doc for why this, not
/// `#[should_panic]`, is the right tool for the error-control-test cases.
fn has_no_committed_geometry(laid: &LaidOut, id: RenderId) -> bool {
    let owner = laid.pipeline_owner();
    flui_rendering::testing::inspect::box_geometry(&owner.read(), id).is_none()
}

/// What the delegate saw, in the order the layout pass asked for it.
///
/// The oracle's `TestMultiChildLayoutDelegate` records the same fields as
/// plain (possibly `late`) fields; FLUI's delegate is `Send + Sync` behind an
/// `Arc`, so the recording lives behind a `Mutex` instead.
#[derive(Debug, Default)]
struct Recording {
    get_size_constraints: Option<BoxConstraints>,
    perform_layout_size: Option<Size>,
    perform_layout_size0: Option<Size>,
    perform_layout_size1: Option<Size>,
    perform_layout_has_fred: Option<bool>,
    should_relayout_called: bool,
}

/// Port of the oracle's `TestMultiChildLayoutDelegate`: a fixed 200×300 own
/// size, loose constraints for each child, and a recording of every argument
/// it was handed. Never calls `positionChild`, matching the oracle.
#[derive(Debug)]
struct RecordingDelegate {
    recording: Arc<Mutex<Recording>>,
    should_relayout_value: bool,
}

impl RecordingDelegate {
    fn recording_pair(
        should_relayout_value: bool,
    ) -> (Arc<dyn MultiChildLayoutDelegate>, Arc<Mutex<Recording>>) {
        let recording = Arc::new(Mutex::new(Recording::default()));
        let delegate = Arc::new(Self {
            recording: Arc::clone(&recording),
            should_relayout_value,
        });
        (delegate, recording)
    }
}

impl MultiChildLayoutDelegate for RecordingDelegate {
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        self.recording
            .lock()
            .expect("recording mutex")
            .get_size_constraints = Some(constraints);
        Size::new(px(200.0), px(300.0))
    }

    fn perform_layout(&self, context: &mut dyn MultiChildLayoutContext, size: Size) {
        let constraints = BoxConstraints::loose(size);
        let size0 = context.layout_child("0", constraints);
        let size1 = context.layout_child("1", constraints);
        let has_fred = context.has_child("fred");

        let mut recording = self.recording.lock().expect("recording mutex");
        recording.perform_layout_size = Some(size);
        recording.perform_layout_size0 = Some(size0);
        recording.perform_layout_size1 = Some(size1);
        recording.perform_layout_has_fred = Some(has_fred);
    }

    fn should_relayout(&self, _old_delegate: &dyn MultiChildLayoutDelegate) -> bool {
        self.recording
            .lock()
            .expect("recording mutex")
            .should_relayout_called = true;
        self.should_relayout_value
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The oracle's `buildFrame`: the subject centred on the test surface, with
/// two `LayoutId`-tagged children — `SizedBox(150×100)` at id `"0"` and
/// `SizedBox(100×200)` at id `"1"`.
fn build_frame(delegate: Arc<dyn MultiChildLayoutDelegate>) -> impl View {
    Center::new().child(CustomMultiChildLayout::new(
        delegate,
        vec![
            LayoutId::new("0", SizedBox::new(150.0, 100.0)).boxed(),
            LayoutId::new("1", SizedBox::new(100.0, 200.0)).boxed(),
        ],
    ))
}

// ============================================================================
// Case 1 — 'Control test for CustomMultiChildLayout'
// ============================================================================

/// The delegate's `get_size` sees the incoming (loosened-by-`Center`)
/// constraints; `perform_layout` is handed the size `get_size` chose
/// (clamped into those constraints, here a no-op clamp since 200×300 already
/// fits); each child comes back at its own fixed size; and a lookup for an id
/// nobody used (`"fred"`) reports absent.
///
/// The numbers are the oracle's: an 800×600 surface reaches the delegate
/// loose (`Center` loosens), `perform_layout` receives 200×300 (the fixed
/// size `get_size` returns), and the two children resolve to 150×100 and
/// 100×200 — their own fixed `SizedBox` sizes, independent of the loose
/// constraints the delegate handed them.
#[test]
fn control_test_delegate_sees_the_incoming_constraints_and_the_laid_out_children() {
    let (delegate, recording) = RecordingDelegate::recording_pair(false);
    let _laid = harness::pump_widget(build_frame(delegate), harness::screen());

    let recording = recording.lock().expect("recording mutex");

    let get_size_constraints = recording
        .get_size_constraints
        .expect("get_size must be called during layout");
    assert_eq!(
        (
            get_size_constraints.min_width,
            get_size_constraints.max_width,
            get_size_constraints.min_height,
            get_size_constraints.max_height,
        ),
        (px(0.0), px(800.0), px(0.0), px(600.0)),
        "get_size receives the incoming constraints, loosened by Center",
    );

    assert_eq!(
        recording.perform_layout_size,
        Some(Size::new(px(200.0), px(300.0))),
        "perform_layout receives the size get_size chose",
    );
    assert_eq!(
        recording.perform_layout_size0,
        Some(Size::new(px(150.0), px(100.0))),
        "child \"0\" resolves to its own fixed SizedBox size",
    );
    assert_eq!(
        recording.perform_layout_size1,
        Some(Size::new(px(100.0), px(200.0))),
        "child \"1\" resolves to its own fixed SizedBox size",
    );
    assert_eq!(
        recording.perform_layout_has_fred,
        Some(false),
        "has_child(\"fred\") reports absent -- no such id was ever attached",
    );
}

// ============================================================================
// Case 2 — 'Test MultiChildDelegate shouldRelayout method'
// ============================================================================

/// Nothing to compare against on the first layout, so the delegate is not
/// asked.
#[test]
fn should_relayout_is_not_called_on_the_first_layout() {
    let (delegate, recording) = RecordingDelegate::recording_pair(false);
    let _laid = harness::pump_widget(build_frame(delegate), harness::screen());

    let recording = recording.lock().expect("recording mutex");
    assert!(
        recording.perform_layout_size.is_some(),
        "layout ran, because the delegate was set for the first time",
    );
    assert!(
        !recording.should_relayout_called,
        "there is no previous delegate to compare against",
    );
}

/// Replacing the delegate asks the new one whether the old one's layout
/// still stands.
#[test]
fn should_relayout_is_consulted_when_the_delegate_is_replaced() {
    let (first, _first_recording) = RecordingDelegate::recording_pair(false);
    let mut laid = harness::pump_widget(build_frame(first), harness::screen());

    let (second, second_recording) = RecordingDelegate::recording_pair(true);
    laid.pump_widget(build_frame(second));

    assert!(
        second_recording
            .lock()
            .expect("recording mutex")
            .should_relayout_called,
        "the incoming delegate is asked about the outgoing one",
    );
}

/// **Divergence pin, asserting the oracle.**
///
/// Flutter skips the relayout when `shouldRelayout` answers false, so
/// `perform_layout` never runs again after the swap. FLUI reaches it anyway —
/// see "Divergence found by this port" above for the two-part gap
/// (`update_render_object` discarding `set_delegate`'s verdict, and
/// `RenderBehavior::on_update` marking needs-layout unconditionally). Ignored
/// rather than inverted: writing the assertion the other way round would lock
/// today's behaviour in as the expected result and turn the eventual fix into
/// a red "regression".
#[test]
#[ignore = "divergence pin: needs BOTH the discarded set_delegate verdict and \
            the unconditional needs-layout mark in RenderBehavior::on_update \
            fixed -- see the should_relayout gap in docs/ROADMAP.md (same gap \
            custom_single_child_layout_test.rs already pins for the \
            single-child delegate)"]
fn a_false_should_relayout_prevents_relayout_pin() {
    let (first, _first_recording) = RecordingDelegate::recording_pair(false);
    let mut laid = harness::pump_widget(build_frame(first), harness::screen());

    let (second, second_recording) = RecordingDelegate::recording_pair(false);
    laid.pump_widget(build_frame(second));

    let recording = second_recording.lock().expect("recording mutex");
    assert!(
        recording.should_relayout_called,
        "precondition: the delegate was consulted at all",
    );
    assert!(
        recording.perform_layout_size.is_none(),
        "the oracle skips layout when should_relayout answers false, so \
         perform_layout must never run again after the swap",
    );
}

// ============================================================================
// Case 3 — 'Nested CustomMultiChildLayouts'
// ============================================================================

/// The oracle's nested tree: an outer `CustomMultiChildLayout` whose id `"0"`
/// child is *another* `CustomMultiChildLayout` (both driven by the *same*
/// delegate instance), and whose id `"1"` child is a plain `SizedBox`.
fn build_nested_frame(delegate: Arc<dyn MultiChildLayoutDelegate>) -> impl View {
    Center::new().child(CustomMultiChildLayout::new(
        Arc::clone(&delegate),
        vec![
            LayoutId::new(
                "0",
                CustomMultiChildLayout::new(
                    Arc::clone(&delegate),
                    vec![
                        LayoutId::new("0", SizedBox::new(150.0, 100.0)).boxed(),
                        LayoutId::new("1", SizedBox::new(100.0, 200.0)).boxed(),
                    ],
                ),
            )
            .boxed(),
            LayoutId::new("1", SizedBox::new(100.0, 200.0)).boxed(),
        ],
    ))
}

/// Reusing one delegate instance for both the outer and inner
/// `CustomMultiChildLayout` lays out every level independently and correctly.
///
/// Geometry, worked from the same `RecordingDelegate` algorithm (fixed
/// 200×300 own size, loose child constraints) applied at both levels: the
/// outer box is 200×300; its id `"0"` child is the inner
/// `CustomMultiChildLayoutBox`, itself 200×300 (its own `get_size` ignores
/// the incoming loose(200,300) constraints and returns the same fixed
/// 200×300, which already fits); the inner box's own two children resolve to
/// their fixed sizes, 150×100 and 100×200; and the outer's id `"1"` child is
/// a fixed 100×200 `SizedBox`.
#[test]
fn nested_layouts_reusing_the_same_delegate_instance_lay_out_independently() {
    let (delegate, _recording) = RecordingDelegate::recording_pair(false);
    let laid = harness::pump_widget(build_nested_frame(delegate), harness::screen());

    let center = laid.find_by_render_type("RenderCenter");
    let outer = laid.only_child(center);
    assert_eq!(
        laid.size(outer),
        Size::new(px(200.0), px(300.0)),
        "the outer CustomMultiChildLayoutBox sizes itself from the delegate",
    );

    let inner = laid.child(outer, 0);
    assert_eq!(
        laid.size(inner),
        Size::new(px(200.0), px(300.0)),
        "the inner CustomMultiChildLayoutBox, laid out through the SAME \
         delegate instance, resolves independently of the outer call already \
         in progress",
    );

    let inner_child_0 = laid.child(inner, 0);
    assert_eq!(
        laid.size(inner_child_0),
        Size::new(px(150.0), px(100.0)),
        "inner id \"0\" keeps its own fixed size",
    );
    let inner_child_1 = laid.child(inner, 1);
    assert_eq!(
        laid.size(inner_child_1),
        Size::new(px(100.0), px(200.0)),
        "inner id \"1\" keeps its own fixed size",
    );

    let outer_child_1 = laid.child(outer, 1);
    assert_eq!(
        laid.size(outer_child_1),
        Size::new(px(100.0), px(200.0)),
        "outer id \"1\" keeps its own fixed size, unaffected by the nested call",
    );
}

// ============================================================================
// Case 4 — 'Loose constraints'
// ============================================================================

/// Port of the oracle's `PreferredSizeDelegate`: a fixed own size, no
/// children touched, and `should_relayout` comparing that size across swaps.
#[derive(Debug)]
struct PreferredSizeDelegate {
    preferred_size: Size,
}

impl MultiChildLayoutDelegate for PreferredSizeDelegate {
    fn get_size(&self, _constraints: BoxConstraints) -> Size {
        self.preferred_size
    }

    fn perform_layout(&self, _context: &mut dyn MultiChildLayoutContext, _size: Size) {}

    fn should_relayout(&self, old_delegate: &dyn MultiChildLayoutDelegate) -> bool {
        old_delegate
            .as_any()
            .downcast_ref::<Self>()
            .is_none_or(|old| old.preferred_size != self.preferred_size)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn build_preferred_size_frame(preferred_size: Size) -> impl View {
    Center::new().child(CustomMultiChildLayout::new(
        Arc::new(PreferredSizeDelegate { preferred_size }),
        Vec::<BoxedView>::new(),
    ))
}

/// A delegate with no children at all still drives the render object's size,
/// and a replacement delegate with a different preferred size resizes it on
/// the next pump.
#[test]
fn replacing_the_delegate_resizes_the_render_object_with_no_children() {
    let mut laid = harness::pump_widget(
        build_preferred_size_frame(Size::new(px(300.0), px(200.0))),
        harness::screen(),
    );

    let subject = laid.find_by_render_type("RenderCustomMultiChildLayoutBox");
    assert_eq!(
        laid.size(subject),
        Size::new(px(300.0), px(200.0)),
        "the render object takes the size the delegate reports",
    );

    laid.pump_widget(build_preferred_size_frame(Size::new(px(350.0), px(250.0))));

    let subject = laid.find_by_render_type("RenderCustomMultiChildLayoutBox");
    assert_eq!(
        laid.size(subject),
        Size::new(px(350.0), px(250.0)),
        "and follows it when a differently-sized delegate replaces it",
    );
}

// ============================================================================
// Cases 6–12 — `group('performLayout error control test')`
// ============================================================================

/// A single `LayoutId`-tagged child, matching the oracle's
/// `buildSingleChildFrame`.
fn build_single_child_frame(delegate: Arc<dyn MultiChildLayoutDelegate>) -> impl View {
    Center::new().child(CustomMultiChildLayout::new(
        delegate,
        vec![LayoutId::new("0", SizedBox::shrink()).boxed()],
    ))
}

/// Lays out children `"0"` and `"1"`. Used bare for case 6 (single-child
/// widget with no `"1"`) and combined with extra untouched children for cases
/// 11 and 12 (leaves some ids never laid out) -- the oracle's own
/// `ZeroAndOneIdLayoutDelegate` is reused across exactly this same set of
/// cases.
#[derive(Debug)]
struct ZeroAndOneIdLayoutDelegate;

impl MultiChildLayoutDelegate for ZeroAndOneIdLayoutDelegate {
    fn perform_layout(&self, context: &mut dyn MultiChildLayoutContext, size: Size) {
        let constraints = BoxConstraints::loose(size);
        context.layout_child("0", constraints);
        context.layout_child("1", constraints);
    }

    fn should_relayout(&self, _old_delegate: &dyn MultiChildLayoutDelegate) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Port of the oracle's `ZeroAndOneIdLayoutDelegate`, case 'layoutChild on
/// non existent child': the delegate lays out ids `"0"` and `"1"`, but the
/// widget only supplies `"0"`. `DelegateLayoutContext::index_for`
/// (`crates/flui-objects/src/layout/custom_multi_child_layout.rs`) panics
/// looking up `"1"`; the pipeline catches it and never commits the subject's
/// geometry (see the module doc's "How a delegate contract violation
/// actually surfaces in FLUI").
#[test]
fn layout_child_on_a_non_existent_id_leaves_the_subject_unlaidout() {
    let laid = harness::pump_widget(
        build_single_child_frame(Arc::new(ZeroAndOneIdLayoutDelegate)),
        harness::screen(),
    );
    let subject = laid.find_by_render_type("RenderCustomMultiChildLayoutBox");
    assert!(
        has_no_committed_geometry(&laid, subject),
        "a delegate that looks up a non-existent child id must never commit \
         a valid geometry for the subject",
    );
}

/// Port of the oracle's `DuplicateLayoutDelegate`, case 'layoutChild more
/// than once': lays out id `"0"` twice.
#[derive(Debug)]
struct DuplicateLayoutDelegate;

impl MultiChildLayoutDelegate for DuplicateLayoutDelegate {
    fn perform_layout(&self, context: &mut dyn MultiChildLayoutContext, size: Size) {
        let constraints = BoxConstraints::loose(size);
        context.layout_child("0", constraints);
        context.layout_child("0", constraints);
    }

    fn should_relayout(&self, _old_delegate: &dyn MultiChildLayoutDelegate) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[test]
fn layout_child_called_twice_for_the_same_id_leaves_the_subject_unlaidout() {
    let laid = harness::pump_widget(
        build_single_child_frame(Arc::new(DuplicateLayoutDelegate)),
        harness::screen(),
    );
    let subject = laid.find_by_render_type("RenderCustomMultiChildLayoutBox");
    assert!(
        has_no_committed_geometry(&laid, subject),
        "a delegate that lays out the same child id twice must never commit \
         a valid geometry for the subject",
    );
}

/// Port of the oracle's `InvalidConstraintsChildLayoutDelegate`, case
/// 'layoutChild on invalid size constraint': hands the child an inverted
/// `BoxConstraints::loose(Size(-1, -1))` (`min_width = 0 > max_width = -1`,
/// same for height).
#[derive(Debug)]
struct InvalidConstraintsChildLayoutDelegate;

impl MultiChildLayoutDelegate for InvalidConstraintsChildLayoutDelegate {
    fn perform_layout(&self, context: &mut dyn MultiChildLayoutContext, _size: Size) {
        let constraints = BoxConstraints::loose(Size::new(px(-1.0), px(-1.0)));
        context.layout_child("0", constraints);
    }

    fn should_relayout(&self, _old_delegate: &dyn MultiChildLayoutDelegate) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Unlike cases 6, 7, 9, 10, 11, this checks the **child's** committed
/// geometry, not the subject's: the invalid constraints reach the child
/// (FLUI's `DelegateLayoutContext::layout_child` performs no validation at
/// all) and it is the CHILD's own layout code that ends up poisoned, not the
/// `CustomMultiChildLayoutBox` itself — confirmed empirically, see "Divergence
/// found by this port" in the module doc for the full explanation of why this
/// passes today for an incidental, fragile reason rather than a validated
/// contract.
#[test]
fn layout_child_with_an_inverted_size_constraint_leaves_the_child_unlaidout() {
    let laid = harness::pump_widget(
        build_single_child_frame(Arc::new(InvalidConstraintsChildLayoutDelegate)),
        harness::screen(),
    );
    let subject = laid.find_by_render_type("RenderCustomMultiChildLayoutBox");
    let child = laid.child(subject, 0);
    assert!(
        has_no_committed_geometry(&laid, child),
        "a child handed an inverted (invalid) box constraint must never \
         commit a valid geometry from it",
    );
}

/// Port of the oracle's `NonExistentPositionDelegate`, case 'positionChild on
/// non existent child': positions id `"0"` (which exists) and then id `"1"`
/// (which does not).
#[derive(Debug)]
struct NonExistentPositionDelegate;

impl MultiChildLayoutDelegate for NonExistentPositionDelegate {
    fn perform_layout(&self, context: &mut dyn MultiChildLayoutContext, _size: Size) {
        context.position_child("0", Offset::ZERO);
        context.position_child("1", Offset::ZERO);
    }

    fn should_relayout(&self, _old_delegate: &dyn MultiChildLayoutDelegate) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[test]
fn position_child_on_a_non_existent_id_leaves_the_subject_unlaidout() {
    let laid = harness::pump_widget(
        build_single_child_frame(Arc::new(NonExistentPositionDelegate)),
        harness::screen(),
    );
    let subject = laid.find_by_render_type("RenderCustomMultiChildLayoutBox");
    assert!(
        has_no_committed_geometry(&laid, subject),
        "a delegate that positions a non-existent child id must never \
         commit a valid geometry for the subject",
    );
}

/// Port of `"_callPerformLayout on child that doesn't have id"`: a child of
/// `CustomMultiChildLayout` that was never wrapped in `LayoutId` carries
/// `MultiChildLayoutParentData::default()` (`id: None`), and
/// `RenderCustomMultiChildLayoutBox::child_slots` rejects that before the
/// delegate's `perform_layout` ever runs.
///
/// Asserting the absent geometry alone would NOT guard that validation.
/// Every layout violation in this render object funnels into the same
/// poisoned-node state, so a delegate that lays out nothing would produce an
/// identical observable through the forgotten-child path instead — the test
/// would pass with the missing-id check deleted. The diagnostic text is what
/// separates the two causes, so that is what this asserts.
#[test]
fn a_child_without_a_layout_id_names_the_missing_id_in_the_captured_log() {
    let (laid, log_text) = pump_and_capture_log(|| {
        Center::new()
            .child(CustomMultiChildLayout::new(
                Arc::new(PreferredSizeDelegate {
                    preferred_size: Size::new(px(10.0), px(10.0)),
                }),
                vec![SizedBox::new(100.0, 100.0).boxed()],
            ))
            .boxed()
    });

    let subject = laid.find_by_render_type("RenderCustomMultiChildLayoutBox");
    assert!(
        has_no_committed_geometry(&laid, subject),
        "a child with no LayoutId-supplied id must never let the subject \
         commit a valid geometry",
    );
    assert!(
        log_text.contains("must have an id in its parent data"),
        "the diagnostic must name the missing-id cause verbatim, not merely \
         report some layout failure — absent geometry alone is also what the \
         forgotten-child path produces (whose diagnostic reads `missing id`), \
         so without this the test would pass with the id validation deleted; \
         captured log: {log_text}",
    );
}

/// Serializes the two tests that install a capturing `tracing` subscriber
/// against each other, and documents a hard requirement: **this technique
/// is only race-free under `cargo nextest run`, never under plain `cargo
/// test`.**
///
/// `tracing::subscriber::with_default` only redirects *event dispatch* for
/// the current thread; the crate's per-callsite "is anyone interested"
/// cache is process-global. Under plain `cargo test` (thread-parallel, every
/// test sharing one process) this was confirmed empirically to be
/// double-broken, not single-broken: without any lock,
/// `a_child_never_laid_out_names_it_in_the_captured_log` intermittently
/// captured an EMPTY log (the render pipeline's own `tracing::error!` call
/// site decided nobody was listening and dropped the event before it ever
/// reached this subscriber); WITH this lock added, the failure mode changed
/// to intermittently capturing text from an *unrelated concurrently-running
/// test's* widget mount instead. Neither failure mode is reachable under
/// `cargo nextest run`: nextest re-invokes the compiled test binary once per
/// test with `--exact <name>`, so every `#[test]` fn gets its own OS
/// process and there is no other test's tracing activity in the same
/// process to race against at all — the same precondition this codebase
/// already leans on for flui-app's genuinely process-global ambient state
/// (see the "Testing Quirks" section of `AGENTS.md`, and the ambient-reach
/// ratchet in `docs/runtime-contract.toml` for the named residuals such as
/// `global_timer_service`/`Registry::global`/`FONT_SYSTEM` — `AppBinding`
/// and `Scheduler` are no longer singletons at all, so there is no lock left
/// to name for them). This lock still
/// serializes these two tests against each other as a defensive backstop
/// (e.g. a future `--test-threads>1` invocation scoped to just this file),
/// but it does not and cannot substitute for nextest's process isolation —
/// run this file's tracing-capturing tests via `cargo nextest run`, not
/// `cargo test`.
static TRACING_CAPTURE_LOCK: Mutex<()> = Mutex::new(());

/// Installs a capturing `tracing` subscriber for the duration of `pump`,
/// returning both the laid-out tree and the diagnostic text the render
/// pipeline logged while pumping it. Thread-local for exactly this call
/// (`tracing::subscriber::with_default`) — never a process-global
/// subscriber — but see [`TRACING_CAPTURE_LOCK`] for the hard
/// nextest-only-isolation requirement this still carries.
fn pump_and_capture_log(build: impl FnOnce() -> BoxedView) -> (LaidOut, String) {
    #[derive(Clone, Default)]
    struct CapturedLog(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for CapturedLog {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("captured-log mutex")
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedLog {
        type Writer = Self;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    let _guard = TRACING_CAPTURE_LOCK.lock().expect("tracing-capture lock");

    let captured = CapturedLog::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(captured.clone())
        .with_max_level(tracing::level_filters::LevelFilter::TRACE)
        .with_ansi(false)
        .without_time()
        .finish();

    let root = build();
    let laid = tracing::subscriber::with_default(subscriber, || {
        harness::pump_widget(root, harness::screen())
    });

    let bytes = captured.0.lock().expect("captured-log mutex").clone();
    let text = String::from_utf8(bytes).expect("tracing output is valid UTF-8");
    (laid, text)
}

/// Port of 'performLayout did not layout a child': three children (`"0"`,
/// `"1"`, `"2"`), the same `ZeroAndOneIdLayoutDelegate` from case 6 lays out
/// only the first two, leaving `"2"` forgotten. The oracle names the single
/// forgotten child; this checks the captured diagnostic names it too (the
/// same capture mechanism case 12's pin needs, applied here where FLUI's
/// message is complete).
#[test]
fn a_child_never_laid_out_names_it_in_the_captured_log() {
    let (laid, log_text) = pump_and_capture_log(|| {
        Center::new()
            .child(CustomMultiChildLayout::new(
                Arc::new(ZeroAndOneIdLayoutDelegate),
                vec![
                    LayoutId::new("0", SizedBox::new(100.0, 100.0)).boxed(),
                    LayoutId::new("1", SizedBox::new(100.0, 100.0)).boxed(),
                    LayoutId::new("2", SizedBox::new(100.0, 100.0)).boxed(),
                ],
            ))
            .boxed()
    });

    let subject = laid.find_by_render_type("RenderCustomMultiChildLayoutBox");
    assert!(
        has_no_committed_geometry(&laid, subject),
        "precondition: leaving a child unlaid must never commit a valid \
         geometry for the subject",
    );
    assert!(
        log_text.contains(r#"missing id \"2\""#),
        "the forgotten child's id must be named in the diagnostic: got {log_text:?}",
    );
}

/// **Divergence pin, asserting the oracle.**
///
/// Port of 'performLayout did not layout multiple child': four children
/// (`"0"`..`"3"`), the same delegate lays out only `"0"` and `"1"`, leaving
/// BOTH `"2"` and `"3"` forgotten. The oracle's message enumerates every
/// forgotten child; see "Divergence found by this port" above for why FLUI's
/// `DelegateLayoutContext::finish` names only the first (`"2"`) and never
/// mentions `"3"` at all.
#[test]
#[ignore = "divergence pin: DelegateLayoutContext::finish only names the \
            FIRST unlaid-out child (Iterator::position stops at the first \
            match); the oracle's message enumerates every forgotten child -- \
            see 'Divergence found by this port' in the module doc"]
fn multiple_children_never_laid_out_are_all_named_in_the_panic_pin() {
    let (laid, log_text) = pump_and_capture_log(|| {
        Center::new()
            .child(CustomMultiChildLayout::new(
                Arc::new(ZeroAndOneIdLayoutDelegate),
                vec![
                    LayoutId::new("0", SizedBox::new(100.0, 100.0)).boxed(),
                    LayoutId::new("1", SizedBox::new(100.0, 100.0)).boxed(),
                    LayoutId::new("2", SizedBox::new(100.0, 100.0)).boxed(),
                    LayoutId::new("3", SizedBox::new(100.0, 100.0)).boxed(),
                ],
            ))
            .boxed()
    });

    let subject = laid.find_by_render_type("RenderCustomMultiChildLayoutBox");
    assert!(
        has_no_committed_geometry(&laid, subject),
        "precondition: leaving children unlaid must never commit a valid \
         geometry for the subject",
    );
    for forgotten_id in ["2", "3"] {
        assert!(
            log_text.contains(&format!("missing id \\\"{forgotten_id}\\\"")),
            "the oracle names every forgotten child, not just the first \
             (checking for {forgotten_id:?}): got {log_text:?}",
        );
    }
}
