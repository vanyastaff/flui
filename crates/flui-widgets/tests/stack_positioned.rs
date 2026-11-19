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
/// overflows them, by default and with any clipping behavior, and let it paint
/// outside with `Clip::None`, at creation and on every rebuild. The child keeps
/// its layout either way.
pub(crate) fn a_stack_clips_overflow_unless_its_clip_behavior_is_none() {
    use crate::common::LaidOut;
    use flui_painting::paint::Clip;
    use flui_rendering::layer::Layer;
    use flui_view::ViewExt;
    use flui_widgets::IndexedStack;

    fn clips(laid: &LaidOut) -> usize {
        laid.layer_tree().map_or(0, |tree| {
            tree.iter()
                .filter(|(_, node)| {
                    matches!(node.layer(), Layer::ClipRect(clip) if clip.clip_behavior() != Clip::None)
                })
                .count()
        })
    }
    // A 200×200 stack whose positioned child lies wholly past its right edge.
    let beyond = || {
        Positioned::new(SizedBox::new(40.0, 40.0))
            .left(220.0)
            .top(0.0)
    };
    // `None` builds the widget with its default clip, `Clip::HardEdge`.
    let sequence = [
        Some(Clip::None),
        None,
        Some(Clip::None),
        Some(Clip::AntiAlias),
    ];
    let clipped = |clip: Option<Clip>| clip != Some(Clip::None);

    let stack = |clip: Option<Clip>| {
        let stack = Stack::new((beyond(),));
        match clip {
            Some(clip) => stack.clip_behavior(clip),
            None => stack,
        }
    };
    let mut laid = lay_out(stack(sequence[0]), tight(200.0, 200.0));
    for (index, &clip) in sequence.iter().enumerate() {
        if index > 0 {
            laid.pump_widget(stack(clip));
        }
        assert_eq!(
            clips(&laid),
            usize::from(clipped(clip)),
            "Stack with {clip:?}"
        );
        let child = laid.only_child(laid.root());
        assert_eq!(
            laid.offset(child),
            offset(220.0, 0.0),
            "Stack with {clip:?}"
        );
    }

    let indexed = |clip: Option<Clip>| {
        let stack = IndexedStack::new(vec![beyond().boxed()]);
        match clip {
            Some(clip) => stack.clip_behavior(clip),
            None => stack,
        }
    };
    let mut laid = lay_out(indexed(sequence[0]), tight(200.0, 200.0));
    for (index, &clip) in sequence.iter().enumerate() {
        if index > 0 {
            laid.pump_widget(indexed(clip));
        }
        assert_eq!(
            clips(&laid),
            usize::from(clipped(clip)),
            "IndexedStack with {clip:?}"
        );
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
