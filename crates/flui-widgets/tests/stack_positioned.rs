//! `Positioned` parent-data parity — a `Positioned` child contributes a
//! `StackParentData` (edge insets + explicit size) that `RenderStack` reads to
//! place and stretch the child. Proves the ParentDataView seam for the Stack
//! family end-to-end (offsets and resolved sizes).

use crate::common::{lay_out, offset, size, tight};
use flui_foundation::TextDirection;
use flui_widgets::{Directionality, Positioned, SizedBox, Stack};

pub(crate) fn positioned_places_child_at_explicit_edges() {
    // A 200×200 stack with a 50×50 child pinned 10 from the left, 20 from top.
    let laid = lay_out(
        Stack::new((Positioned::new(SizedBox::new(50.0, 50.0))
            .left(10.0)
            .top(20.0),)),
        tight(200.0, 200.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(200.0, 200.0));

    let child = laid.only_child(root);
    assert_eq!(laid.size(child), size(50.0, 50.0));
    assert_eq!(laid.offset(child), offset(10.0, 20.0));
}

/// `Positioned::directional`'s `start`/`end` resolve against the ambient `Directionality`:
/// `start` is the left edge in LTR and the right edge in RTL.
pub(crate) fn positioned_start_resolves_against_directionality() {
    let child = || {
        Positioned::directional(SizedBox::new(50.0, 50.0))
            .start(10.0)
            .top(20.0)
    };

    let ltr = lay_out(
        Directionality::new(TextDirection::Ltr, Stack::new((child(),))),
        tight(200.0, 200.0),
    );
    assert_eq!(
        ltr.offset(ltr.only_child(ltr.root())),
        offset(10.0, 20.0),
        "start = left in LTR"
    );

    let rtl = lay_out(
        Directionality::new(TextDirection::Rtl, Stack::new((child(),))),
        tight(200.0, 200.0),
    );
    assert_eq!(
        rtl.offset(rtl.only_child(rtl.root())),
        offset(140.0, 20.0),
        "start = right in RTL: 200 - 10 - 50"
    );
}

// ============================================================================
// Stack itself: non-positioned children -- sizing to the largest, and
// alignment. Every test above uses `Positioned` children exclusively;
// `Stack`'s own default (`Alignment::TOP_LEFT`, `StackFit::Loose`) and
// explicit-alignment/fit behavior with plain non-positioned children were
// never directly exercised.
// ============================================================================

/// `Stack` and `IndexedStack` clip the paint of a positioned child that
/// overflows them to their own box, with the behavior asked for (`HardEdge`
/// by default), and let it paint outside with `Clip::None`, at creation and on
/// every rebuild. A child inside the box is not clipped, and the child keeps
/// its layout either way. A pointer outside the stack reaches nothing, even
/// with `Clip::None`.
pub(crate) fn a_stack_clips_overflow_unless_its_clip_behavior_is_none() {
    use crate::common::LaidOut;
    use flui_foundation::geometry::{Offset, Point, Rect};
    use flui_interaction::events::PointerKind;
    use flui_layer::Layer;
    use flui_painting::paint::Clip;
    use flui_view::ViewExt;
    use flui_widgets::IndexedStack;

    /// The clip layers that clip: each one's behavior and rect.
    fn clips(laid: &LaidOut) -> Vec<(Clip, Rect<f64>)> {
        laid.layer_tree().map_or_else(Vec::new, |tree| {
            tree.iter()
                .filter_map(|(_, node)| match node.layer() {
                    Layer::ClipRect(clip) if clip.clip_behavior() != Clip::None => {
                        Some((clip.clip_behavior(), clip.clip_rect()))
                    }
                    _ => None,
                })
                .collect()
        })
    }
    let own_box = Rect::from_origin_size(Point::ZERO, size(200.0, 200.0));
    // A 40×40 positioned child of a 200×200 stack at `(left, top)`.
    let at = |left: f64, top: f64| {
        Positioned::new(SizedBox::new(40.0, 40.0))
            .left(left)
            .top(top)
    };
    // (child position, overflows). `None` as a clip builds the widget with
    // its default, `Clip::HardEdge`.
    let positions = [
        ((220.0, 0.0), true),  // wholly past the right edge
        ((180.0, 10.0), true), // straddling the right edge
        ((-10.0, 10.0), true), // a negative offset past the left edge
        ((80.0, 80.0), false), // inside the box
    ];
    let sequence = [
        Some(Clip::None),
        None,
        Some(Clip::None),
        Some(Clip::AntiAlias),
    ];
    let expected = |clip: Option<Clip>, overflows: bool| match clip {
        Some(Clip::None) => vec![],
        _ if !overflows => vec![],
        clip => vec![(clip.unwrap_or(Clip::HardEdge), own_box)],
    };

    for ((left, top), overflows) in positions {
        let stack = |clip: Option<Clip>| {
            let stack = Stack::new((at(left, top),));
            match clip {
                Some(clip) => stack.clip_behavior(clip),
                None => stack,
            }
        };
        let indexed = |clip: Option<Clip>| {
            let stack = IndexedStack::new(vec![at(left, top).boxed()]);
            match clip {
                Some(clip) => stack.clip_behavior(clip),
                None => stack,
            }
        };
        let mut stacked = lay_out(stack(sequence[0]), tight(200.0, 200.0));
        let mut shown = lay_out(indexed(sequence[0]), tight(200.0, 200.0));
        for (index, &clip) in sequence.iter().enumerate() {
            if index > 0 {
                stacked.pump_widget(stack(clip));
                shown.pump_widget(indexed(clip));
            }
            for (name, laid) in [("Stack", &stacked), ("IndexedStack", &shown)] {
                let case = format!("{name} with {clip:?}, child at ({left}, {top})");
                assert_eq!(clips(laid), expected(clip, overflows), "{case}");
                let child = laid.only_child(laid.root());
                assert_eq!(laid.offset(child), offset(left, top), "{case}");
                assert!(
                    laid.hit_test_pointer(Offset::new(230.0, 20.0), PointerKind::Mouse)
                        .path()
                        .is_empty(),
                    "{case}: a pointer outside the stack reaches nothing"
                );
            }
        }
    }
}

/// An indexed stack is heard only through the child it shows, and switching the index
/// switches what is heard.
pub(crate) fn an_indexed_stack_is_heard_only_through_its_shown_child() {
    use flui_view::ViewExt;
    use flui_widgets::{IndexedStack, Semantics};
    let stack = |index| {
        IndexedStack::new(vec![
            Semantics::new()
                .label("first")
                .child(SizedBox::new(20.0, 20.0))
                .boxed(),
            Semantics::new()
                .label("second")
                .child(SizedBox::new(20.0, 20.0))
                .boxed(),
        ])
        .index(Some(index))
    };
    let mut laid = lay_out(stack(0), tight(100.0, 100.0));
    laid.enable_semantics();
    laid.tick();
    let tree = laid.a11y_tree().expect("semantics enabled");
    assert!(tree.find_by_label("first").is_ok(), "the shown child");
    assert!(
        tree.find_by_label("second").is_err(),
        "the hidden child is not heard"
    );

    laid.pump_widget(stack(1));
    laid.tick();
    let tree = laid.a11y_tree().expect("semantics enabled");
    assert!(
        tree.find_by_label("second").is_ok(),
        "the newly shown child"
    );
    assert!(
        tree.find_by_label("first").is_err(),
        "the newly hidden child"
    );
}
