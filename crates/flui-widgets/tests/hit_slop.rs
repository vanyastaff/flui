//! `HitSlop` and `ContentShape`: which target a tap reaches when hit areas are
//! expanded or shaped.
//!
//! Resolution order for expanded areas: a visual hit on another target, then
//! the nearest edge, then the topmost; an area never takes a point that hit
//! content in another composited layer or a pointer barrier. Every scenario taps through the UI runtime's real input path and
//! counts the taps each `GestureDetector` recognises.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::common::{LaidOut, lay_out, tight};
use crate::gesture_settings::SettingsScope;
use flui_foundation::geometry::EdgeInsets;
use flui_interaction::events::PointerKind;
use flui_interaction::{GestureSettings, GestureSettingsSource};
use flui_painting::styling::Color;
use flui_platform_api::TargetPlatform;
use flui_view::{IntoView, ViewExt};
use flui_widgets::row;
use flui_widgets::{
    AbsorbPointer, Center, ColoredBox, ContentShape, GestureDetector, HitShape, HitSlop,
    Positioned, RepaintBoundary, SizedBox, Stack,
};

/// A `side`-square tap target that counts its taps.
fn target(side: f64, taps: &Arc<AtomicUsize>) -> GestureDetector {
    let taps = Arc::clone(taps);
    GestureDetector::new()
        .on_tap(move |_cx| {
            taps.fetch_add(1, Ordering::SeqCst);
        })
        .child(SizedBox::new(side, side).child(ColoredBox::new(Color::rgb(10, 20, 30))))
}

fn tap(laid: &LaidOut, x: f64, y: f64) {
    laid.dispatch_pointer_down(x, y);
    laid.dispatch_pointer_up(x, y);
}

fn counts(taps: &[&Arc<AtomicUsize>]) -> Vec<usize> {
    taps.iter().map(|t| t.load(Ordering::SeqCst)).collect()
}

fn page() -> ColoredBox {
    ColoredBox::new(Color::rgb(240, 240, 240))
}

/// Two 20-px targets 30 px apart, each with 30 px of slop, so their areas
/// overlap across the whole gap; the right one is on top.
pub(crate) fn overlapping_slops_go_to_the_nearest_edge_then_the_topmost() {
    let left = Arc::new(AtomicUsize::new(0));
    let right = Arc::new(AtomicUsize::new(0));
    let laid = lay_out(
        Stack::new(row![
            SizedBox::new(200.0, 200.0).child(page()),
            Positioned::new(
                HitSlop::new()
                    .slop(EdgeInsets::all(30.0))
                    .child(target(20.0, &left))
            )
            .left(50.0)
            .top(90.0),
            Positioned::new(
                HitSlop::new()
                    .slop(EdgeInsets::all(30.0))
                    .child(target(20.0, &right)),
            )
            .left(100.0)
            .top(90.0),
        ]),
        tight(200.0, 200.0),
    );

    // 5 px from the left target, 25 px from the right one: nearest wins even
    // though the right one is on top.
    tap(&laid, 75.0, 100.0);
    assert_eq!(counts(&[&left, &right]), [1, 0], "nearest edge");
    // Equidistant (15 px each): the topmost wins.
    tap(&laid, 85.0, 100.0);
    assert_eq!(counts(&[&left, &right]), [1, 1], "topmost on a tie");
    // Beyond both areas: nobody.
    tap(&laid, 10.0, 10.0);
    assert_eq!(counts(&[&left, &right]), [1, 1], "outside every area");
}

/// A target hit visually keeps the tap even when a neighbour drawn above it
/// reaches the same point with its slop.
pub(crate) fn a_visual_hit_beats_a_neighbours_slop() {
    let plain = Arc::new(AtomicUsize::new(0));
    let slopped = Arc::new(AtomicUsize::new(0));
    let laid = lay_out(
        Stack::new(row![
            SizedBox::new(200.0, 200.0).child(page()),
            Positioned::new(target(20.0, &plain)).left(50.0).top(90.0),
            Positioned::new(
                HitSlop::new()
                    .slop(EdgeInsets::all(30.0))
                    .child(target(20.0, &slopped)),
            )
            .left(72.0)
            .top(90.0),
        ]),
        tight(200.0, 200.0),
    );

    tap(&laid, 65.0, 100.0);
    assert_eq!(counts(&[&plain, &slopped]), [1, 0], "visual hit wins");
    // Just right of the plain target, inside only the neighbour's slop.
    tap(&laid, 71.0, 100.0);
    assert_eq!(counts(&[&plain, &slopped]), [1, 1], "slop takes the gap");
}

/// A target in its own composited layer above the page cannot take a tap
/// that visually lands on the page; in the page's layer it can.
pub(crate) fn slop_does_not_leave_its_composited_layer() {
    fn overlay(isolated: bool, taps: &Arc<AtomicUsize>) -> LaidOut {
        let content = Stack::new(row![
            Positioned::new(
                HitSlop::new()
                    .slop(EdgeInsets::all(30.0))
                    .child(target(20.0, taps)),
            )
            .left(90.0)
            .top(90.0),
        ]);
        let layer = if isolated {
            RepaintBoundary::new().child(content).into_view().boxed()
        } else {
            content.into_view().boxed()
        };
        lay_out(
            Stack::new(row![SizedBox::new(200.0, 200.0).child(page()), layer]),
            tight(200.0, 200.0),
        )
    }

    let shared = Arc::new(AtomicUsize::new(0));
    tap(&overlay(false, &shared), 80.0, 100.0);
    assert_eq!(
        counts(&[&shared]),
        [1],
        "same layer: the slop takes the tap"
    );

    let isolated = Arc::new(AtomicUsize::new(0));
    let laid = overlay(true, &isolated);
    tap(&laid, 80.0, 100.0);
    assert_eq!(
        counts(&[&isolated]),
        [0],
        "the tap landed in the page's layer"
    );
    tap(&laid, 100.0, 100.0);
    assert_eq!(counts(&[&isolated]), [1], "a visual hit still works");
}

/// A circular target ignores taps in the corners of its bounding box; they
/// reach what is below.
pub(crate) fn content_shape_rejects_the_corners_of_a_circle() {
    let circle = Arc::new(AtomicUsize::new(0));
    let below = Arc::new(AtomicUsize::new(0));
    let laid = lay_out(
        Stack::new(row![
            target(100.0, &below),
            ContentShape::new(HitShape::Oval).child(target(100.0, &circle)),
        ]),
        tight(100.0, 100.0),
    );

    tap(&laid, 8.0, 8.0);
    assert_eq!(counts(&[&circle, &below]), [0, 1], "corner falls through");
    tap(&laid, 50.0, 50.0);
    assert_eq!(counts(&[&circle, &below]), [1, 1], "centre hits the circle");
}

/// Without an authored size, the minimum target is the gesture settings'
/// `min_target`: 44 px for iOS touch, 24 px for a Windows mouse.
pub(crate) fn min_target_defaults_to_the_settings_preset() {
    fn tap_beside(platform: TargetPlatform, kind: PointerKind) -> usize {
        let taps = Arc::new(AtomicUsize::new(0));
        let laid = lay_out(
            SettingsScope::new(
                GestureSettings::preset(platform, kind),
                Stack::new(row![
                    SizedBox::new(200.0, 200.0).child(page()),
                    Positioned::new(HitSlop::new().child(target(20.0, &taps)))
                        .left(90.0)
                        .top(90.0),
                ]),
            ),
            tight(200.0, 200.0),
        );
        // 10 px left of a 20 px target: inside a 44 px minimum (12 px per
        // side), outside a 24 px one (2 px per side).
        tap(&laid, 80.0, 100.0);
        taps.load(Ordering::SeqCst)
    }

    assert_eq!(tap_beside(TargetPlatform::iOS, PointerKind::Touch), 1);
    assert_eq!(tap_beside(TargetPlatform::Windows, PointerKind::Mouse), 0);
}

/// A host update to the live settings changes the minimum target at the next
/// tap. Publishing schedules no frame and rebuilds nothing.
pub(crate) fn min_target_follows_a_live_settings_update() {
    let source = GestureSettingsSource::new(GestureSettings::preset(
        TargetPlatform::iOS,
        PointerKind::Touch,
    ));
    let taps = Arc::new(AtomicUsize::new(0));
    let laid = lay_out(
        SettingsScope::new(
            source.provider(),
            Stack::new(row![
                SizedBox::new(200.0, 200.0).child(page()),
                Positioned::new(HitSlop::new().child(target(20.0, &taps)))
                    .left(90.0)
                    .top(90.0),
            ]),
        ),
        tight(200.0, 200.0),
    );

    tap(&laid, 80.0, 100.0);
    assert_eq!(counts(&[&taps]), [1], "44 px target");
    assert!(source.replace(GestureSettings::preset(
        TargetPlatform::Windows,
        PointerKind::Mouse
    )));
    tap(&laid, 80.0, 100.0);
    assert_eq!(counts(&[&taps]), [1], "24 px target after the update");
}

/// List items that are each their own layer, with transparent padding between
/// their buttons: a tap in that gap hits only the items' shared ancestors, so
/// it goes to the nearest button's slop. (Item bounds still clip each slop.)
pub(crate) fn slop_takes_the_gap_between_items_in_their_own_layers() {
    let first = Arc::new(AtomicUsize::new(0));
    let second = Arc::new(AtomicUsize::new(0));
    let item = |taps: &Arc<AtomicUsize>| {
        RepaintBoundary::new().child(
            SizedBox::new(60.0, 40.0).child(
                Center::new().child(
                    HitSlop::new()
                        .slop(EdgeInsets::all(30.0))
                        .child(target(20.0, taps)),
                ),
            ),
        )
    };
    let laid = lay_out(
        ColoredBox::new(Color::rgb(250, 250, 250)).child(Stack::new(row![
            // Buttons at x 60..80 and 120..140; the cells meet at x 100.
            Positioned::new(item(&first)).left(40.0).top(80.0),
            Positioned::new(item(&second)).left(100.0).top(80.0),
        ])),
        tight(200.0, 200.0),
    );

    tap(&laid, 90.0, 100.0);
    assert_eq!(counts(&[&first, &second]), [1, 0], "nearer the first");
    tap(&laid, 110.0, 100.0);
    assert_eq!(counts(&[&first, &second]), [1, 1], "nearer the second");
}

/// A modal barrier keeps every tap that lands on it: from a page target's slop
/// beneath it and from a dialog target's slop above it.
pub(crate) fn a_modal_barrier_blocks_slop_on_both_sides() {
    let page_target = Arc::new(AtomicUsize::new(0));
    let dialog_target = Arc::new(AtomicUsize::new(0));
    let slop = |taps: &Arc<AtomicUsize>| {
        HitSlop::new()
            .slop(EdgeInsets::all(30.0))
            .child(target(20.0, taps))
    };
    let laid = lay_out(
        Stack::new(row![
            SizedBox::new(200.0, 200.0).child(page()),
            Positioned::new(slop(&page_target)).left(20.0).top(90.0),
            AbsorbPointer::new().child(SizedBox::new(200.0, 200.0)),
            Positioned::new(slop(&dialog_target)).left(120.0).top(90.0),
        ]),
        tight(200.0, 200.0),
    );

    tap(&laid, 15.0, 100.0);
    tap(&laid, 30.0, 100.0);
    assert_eq!(counts(&[&page_target, &dialog_target]), [0, 0], "beneath");
    tap(&laid, 110.0, 100.0);
    assert_eq!(counts(&[&page_target, &dialog_target]), [0, 0], "above");
    tap(&laid, 130.0, 100.0);
    assert_eq!(
        counts(&[&page_target, &dialog_target]),
        [0, 1],
        "on the dialog"
    );
}
