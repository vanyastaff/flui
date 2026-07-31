//! ## Test parity notes
//!
//! Flutter source:
//! - Widget: `packages/flutter/lib/src/widgets/basic.dart`
//!   `CustomSingleChildLayout` over `SingleChildLayoutDelegate`
//!   (`packages/flutter/lib/src/rendering/shifted_box.dart`).
//! - Render object: `packages/flutter/lib/src/rendering/shifted_box.dart`
//!   `RenderCustomSingleChildLayoutBox`.
//! - Tests: `packages/flutter/test/widgets/custom_single_child_layout_test.dart`
//!   (tag `3.44.0`), all four cases.
//!
//! ## Case ledger
//!
//! 1. `'Control test for CustomSingleChildLayout'` — ported, real green:
//!    [`control_test_delegate_sees_the_incoming_constraints_and_the_laid_out_child`].
//! 2. `'Test SingleChildDelegate shouldRelayout method'` — ported as **three
//!    separate assertions**, and the middle leg is a **pinned divergence**:
//!    [`should_relayout_is_not_called_on_the_first_layout`],
//!    [`should_relayout_is_consulted_when_the_delegate_is_replaced`],
//!    [`a_false_should_relayout_does_not_prevent_relayout_pin`].
//! 3. `'Delegate can change size'` — ported, real green:
//!    [`replacing_the_delegate_resizes_the_render_object`].
//! 4. `'Can use listener for relayout'` — **out of scope by missing feature,
//!    no pin**: the oracle's `SingleChildLayoutDelegate({Listenable? relayout})`
//!    lets a delegate re-layout without being replaced, by driving a
//!    `ValueNotifier` the render object listens to. FLUI's
//!    `SingleChildLayoutDelegate` has no `relayout` member and the render
//!    object subscribes to nothing, so there is no observable to assert on —
//!    a test here could only re-assert case 3 through a different surface.
//!    Adding the listener is a delegate-API change with its own subscribe /
//!    unsubscribe lifecycle across `set_delegate`, not a test gap.
//!
//! ## Divergence found by this port
//!
//! The oracle's case 2 asserts that a `shouldRelayout` returning **false**
//! *suppresses* the relayout: after the delegate swap, `getConstraintsForChild`
//! is never called again. FLUI consults `should_relayout` correctly —
//! `RenderCustomSingleChildLayoutBox::set_delegate` returns exactly Flutter's
//! verdict (`type_changed || delegate.should_relayout(old)`) — but **nothing
//! consumes that verdict**: `CustomSingleChildLayout::update_render_object`
//! discards the returned `bool`, and the render-behavior update path marks
//! needs-layout unconditionally. So the delegate is asked and then ignored,
//! and layout re-runs either way.
//!
//! The visible cost is wasted work rather than a wrong picture: a delegate
//! that answers "nothing changed" still gets a full re-layout, so this is a
//! performance divergence, not a correctness one — which is why it is pinned
//! rather than fixed here. Fixing it means threading the change-flag out of
//! `update_render_object` into the needs-layout decision, which is a shared
//! render-behavior contract touching every delegate-driven render object, not
//! this widget alone.

use std::any::Any;
use std::sync::{Arc, Mutex};

use flui_rendering::constraints::BoxConstraints;
use flui_types::geometry::px;
use flui_types::{Offset, Size};
use flui_widgets::{Center, CustomSingleChildLayout, SingleChildLayoutDelegate, SizedBox};

use crate::harness;

// ============================================================================
// Fixtures
// ============================================================================

/// What the delegate saw, in the order the layout pass asked for it.
///
/// The oracle's `TestSingleChildLayoutDelegate` records the same four values
/// into plain fields; FLUI's delegate is `Send + Sync` behind an `Arc`, so the
/// recording lives behind a `Mutex` instead.
#[derive(Debug, Default)]
struct Recording {
    constraints_from_get_size: Option<BoxConstraints>,
    constraints_from_get_constraints_for_child: Option<BoxConstraints>,
    size_from_get_position_for_child: Option<Size>,
    child_size_from_get_position_for_child: Option<Size>,
    should_relayout_called: bool,
}

/// Port of the oracle's `TestSingleChildLayoutDelegate`: a fixed 200×300 own
/// size, a fixed 100..150 × 200..400 child constraint box, and a recording of
/// every argument it was handed.
#[derive(Debug)]
struct RecordingDelegate {
    recording: Arc<Mutex<Recording>>,
    should_relayout_value: bool,
}

impl RecordingDelegate {
    fn recording_pair(
        should_relayout_value: bool,
    ) -> (Arc<dyn SingleChildLayoutDelegate>, Arc<Mutex<Recording>>) {
        let recording = Arc::new(Mutex::new(Recording::default()));
        let delegate = Arc::new(Self {
            recording: Arc::clone(&recording),
            should_relayout_value,
        });
        (delegate, recording)
    }
}

impl SingleChildLayoutDelegate for RecordingDelegate {
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        self.recording
            .lock()
            .expect("recording mutex")
            .constraints_from_get_size = Some(constraints);
        Size::new(px(200.0), px(300.0))
    }

    fn get_constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        self.recording
            .lock()
            .expect("recording mutex")
            .constraints_from_get_constraints_for_child = Some(constraints);
        BoxConstraints::new(px(100.0), px(150.0), px(200.0), px(400.0))
    }

    fn get_position_for_child(&self, size: Size, child_size: Size) -> Offset {
        let mut recording = self.recording.lock().expect("recording mutex");
        recording.size_from_get_position_for_child = Some(size);
        recording.child_size_from_get_position_for_child = Some(child_size);
        Offset::ZERO
    }

    fn should_relayout(&self, _old_delegate: &dyn SingleChildLayoutDelegate) -> bool {
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

/// Port of the oracle's `FixedSizeLayoutDelegate`: sizes itself and its child
/// to `size`, and relayouts exactly when that size differs.
#[derive(Debug)]
struct FixedSizeLayoutDelegate {
    size: Size,
}

impl SingleChildLayoutDelegate for FixedSizeLayoutDelegate {
    fn get_size(&self, _constraints: BoxConstraints) -> Size {
        self.size
    }

    fn get_constraints_for_child(&self, _constraints: BoxConstraints) -> BoxConstraints {
        BoxConstraints::tight(self.size)
    }

    fn get_position_for_child(&self, _size: Size, _child_size: Size) -> Offset {
        Offset::ZERO
    }

    fn should_relayout(&self, old_delegate: &dyn SingleChildLayoutDelegate) -> bool {
        old_delegate
            .as_any()
            .downcast_ref::<Self>()
            .is_none_or(|old| old.size != self.size)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The oracle's `buildFrame`: the subject centred on the test surface, wrapping
/// a child that takes whatever the delegate's constraints allow.
///
/// The oracle's `Container()` with no arguments expands to the biggest size its
/// constraints permit; `SizedBox::expand()` is the FLUI spelling of that.
fn build_frame(delegate: Arc<dyn SingleChildLayoutDelegate>) -> impl flui_view::View {
    Center::new().child(CustomSingleChildLayout::new(delegate).child(SizedBox::expand()))
}

// ============================================================================
// Case 1 — 'Control test for CustomSingleChildLayout'
// ============================================================================

/// The delegate is handed the **incoming** constraints by both `get_size` and
/// `get_constraints_for_child`, and `get_position_for_child` is handed the size
/// it chose along with the size the child actually took.
///
/// The numbers are the oracle's: an 800×600 surface reaches the delegate loose
/// (`Center` loosens), the delegate answers 200×300 for itself and
/// 100..150 × 200..400 for the child, and the expanding child lands on the
/// child box's maximum, 150×400.
#[test]
fn control_test_delegate_sees_the_incoming_constraints_and_the_laid_out_child() {
    let (delegate, recording) = RecordingDelegate::recording_pair(false);
    let _laid = harness::pump_widget(build_frame(delegate), harness::screen());

    let recording = recording.lock().expect("recording mutex");

    let from_get_size = recording
        .constraints_from_get_size
        .expect("get_size must be called during layout");
    assert_eq!(
        (
            from_get_size.min_width,
            from_get_size.max_width,
            from_get_size.min_height,
            from_get_size.max_height
        ),
        (px(0.0), px(800.0), px(0.0), px(600.0)),
        "get_size receives the incoming constraints unchanged",
    );

    let from_get_constraints = recording
        .constraints_from_get_constraints_for_child
        .expect("get_constraints_for_child must be called when there is a child");
    assert_eq!(
        (
            from_get_constraints.min_width,
            from_get_constraints.max_width,
            from_get_constraints.min_height,
            from_get_constraints.max_height
        ),
        (px(0.0), px(800.0), px(0.0), px(600.0)),
        "get_constraints_for_child receives the SAME incoming constraints, not \
         the size get_size just returned",
    );

    assert_eq!(
        recording.size_from_get_position_for_child,
        Some(Size::new(px(200.0), px(300.0))),
        "get_position_for_child is handed the size the delegate chose",
    );
    assert_eq!(
        recording.child_size_from_get_position_for_child,
        Some(Size::new(px(150.0), px(400.0))),
        "and the size the child actually took — the maximum of the child box, \
         because the child expands",
    );
}

// ============================================================================
// Case 2 — 'Test SingleChildDelegate shouldRelayout method'
// ============================================================================

/// Nothing to compare against on the first layout, so the delegate is not asked.
#[test]
fn should_relayout_is_not_called_on_the_first_layout() {
    let (delegate, recording) = RecordingDelegate::recording_pair(false);
    let _laid = harness::pump_widget(build_frame(delegate), harness::screen());

    let recording = recording.lock().expect("recording mutex");
    assert!(
        recording
            .constraints_from_get_constraints_for_child
            .is_some(),
        "layout ran, because the delegate was set for the first time",
    );
    assert!(
        !recording.should_relayout_called,
        "there is no previous delegate to compare against",
    );
}

/// Replacing the delegate asks the new one whether the old one's layout still
/// stands.
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

/// **Pinned divergence.** The oracle asserts that a `false` answer *suppresses*
/// the relayout — `getConstraintsForChild` is never called again after the swap.
/// FLUI asks the delegate and then lays out regardless.
///
/// `set_delegate` computes Flutter's verdict correctly and returns it; the
/// caller drops it, and the render-behavior update marks needs-layout
/// unconditionally. So this asserts what FLUI does today, and goes red when the
/// change-flag is threaded through — which is the intent.
#[test]
fn a_false_should_relayout_does_not_prevent_relayout_pin() {
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
        recording
            .constraints_from_get_constraints_for_child
            .is_some(),
        "DIVERGENCE from the oracle, pinned: Flutter skips layout when \
         shouldRelayout answers false, so getConstraintsForChild is never \
         reached; FLUI lays out anyway because the verdict is discarded. \
         Wasted work rather than a wrong picture — see the module doc.",
    );
}

// ============================================================================
// Case 3 — 'Delegate can change size'
// ============================================================================

/// A delegate that reports a different size resizes the render object on the
/// next pump.
#[test]
fn replacing_the_delegate_resizes_the_render_object() {
    let mut laid = harness::pump_widget(
        build_frame(Arc::new(FixedSizeLayoutDelegate {
            size: Size::new(px(100.0), px(200.0)),
        })),
        harness::screen(),
    );

    let subject = laid.find_by_render_type("RenderCustomSingleChildLayoutBox");
    assert_eq!(
        laid.size(subject),
        Size::new(px(100.0), px(200.0)),
        "the render object takes the size the delegate reports",
    );

    laid.pump_widget(build_frame(Arc::new(FixedSizeLayoutDelegate {
        size: Size::new(px(150.0), px(240.0)),
    })));

    let subject = laid.find_by_render_type("RenderCustomSingleChildLayoutBox");
    assert_eq!(
        laid.size(subject),
        Size::new(px(150.0), px(240.0)),
        "and follows it when a differently-sized delegate replaces it",
    );
}
