//! Tests for `MetaData` — the widget that makes a subtree findable from a
//! hit-test path by something that knows nothing about it.
//!
//! This is the discovery mechanism drag-target lookup rides on: a drag has a
//! position and no way to ask the element tree who is under it, so the target
//! tags itself and the drag downcasts whatever it finds.

use crate::common::{lay_out, offset, tight};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_widgets::{Listener, MetaData, SizedBox};

#[derive(Debug, PartialEq)]
struct Slot(&'static str);

/// A child that actually claims hits.
///
/// Not a `ColoredBox` over a `SizedBox`: neither is hit-testable — a childless
/// `RenderConstrainedBox` returns false from `forward_hit_test`, and a paint-only
/// proxy inherits that miss. Under `MetaData`'s default `DeferToChild` a tag over
/// such a child is correctly never found, so using one here would have made these
/// tests assert nothing.
fn hittable() -> Listener {
    Listener::new()
        .behavior(HitTestBehavior::Opaque)
        .child(SizedBox::new(40.0, 40.0))
}

/// A payload attached by `MetaData` is found by hit-testing over its child.
#[test]
fn a_tagged_subtree_is_found_by_a_hit_test_over_it() {
    let laid = lay_out(
        MetaData::new(Slot("inbox")).child(hittable()),
        tight(40.0, 40.0),
    );

    let hit = laid.hit_test_pointer(offset(20.0, 20.0));
    let found: Vec<&Slot> = hit
        .path()
        .iter()
        .filter_map(|entry| entry.metadata_as::<Slot>())
        .collect();

    assert_eq!(
        found,
        vec![&Slot("inbox")],
        "a hit over the tagged child must carry the payload back out"
    );
}

/// A payload of a different type is not mistaken for the one being sought.
#[test]
fn a_payload_of_another_type_is_not_returned() {
    #[derive(Debug, PartialEq)]
    struct Other(u32);

    let laid = lay_out(MetaData::new(Other(7)).child(hittable()), tight(40.0, 40.0));
    let hit = laid.hit_test_pointer(offset(20.0, 20.0));

    assert!(
        !hit.path().is_empty(),
        "premise: the position hits, so the absence below is about the type \
         and not about the hit"
    );
    assert!(
        hit.path()
            .iter()
            .all(|entry| entry.metadata_as::<Slot>().is_none()),
        "downcasting to the wrong type must find nothing, not the payload"
    );
    assert_eq!(
        hit.path()
            .iter()
            .filter_map(|entry| entry.metadata_as::<Other>())
            .collect::<Vec<_>>(),
        vec![&Other(7)],
        "...while its own type still finds it — otherwise this test would \
         pass against a MetaData that attaches nothing at all"
    );
}

/// The default `DeferToChild` means an empty tag is not in the path.
///
/// `MetaData`'s behaviour decides whether it is findable at all, and the
/// default follows its child. A tag over empty space is invisible unless it
/// asks to be opaque — which is a real footgun for a drop target sized larger
/// than its contents, so both halves are pinned.
#[test]
fn behavior_decides_whether_an_empty_region_is_findable() {
    let defer = lay_out(
        MetaData::new(Slot("defer")).child(SizedBox::new(40.0, 40.0)),
        tight(40.0, 40.0),
    );
    let defer_hit = defer.hit_test_pointer(offset(20.0, 20.0));
    assert!(
        defer_hit
            .path()
            .iter()
            .all(|entry| entry.metadata_as::<Slot>().is_none()),
        "a bare SizedBox is not hit-testable, and DeferToChild inherits that \
         miss — the tag is not found"
    );

    let opaque = lay_out(
        MetaData::new(Slot("opaque"))
            .behavior(HitTestBehavior::Opaque)
            .child(SizedBox::new(40.0, 40.0)),
        tight(40.0, 40.0),
    );
    let opaque_hit = opaque.hit_test_pointer(offset(20.0, 20.0));
    assert_eq!(
        opaque_hit
            .path()
            .iter()
            .filter_map(|entry| entry.metadata_as::<Slot>())
            .collect::<Vec<_>>(),
        vec![&Slot("opaque")],
        "an opaque tag claims its own bounds and is found over empty space"
    );
}

/// Nested tags come back leaf-first, so the innermost target wins.
#[test]
fn nested_tags_come_back_innermost_first() {
    let laid = lay_out(
        MetaData::new(Slot("outer"))
            .behavior(HitTestBehavior::Opaque)
            .child(
                MetaData::new(Slot("inner"))
                    .behavior(HitTestBehavior::Opaque)
                    .child(hittable()),
            ),
        tight(40.0, 40.0),
    );

    let hit = laid.hit_test_pointer(offset(20.0, 20.0));
    let found: Vec<&Slot> = hit
        .path()
        .iter()
        .filter_map(|entry| entry.metadata_as::<Slot>())
        .collect();

    assert_eq!(
        found,
        vec![&Slot("inner"), &Slot("outer")],
        "the hit path is leaf-first, so a searcher taking the first match \
         gets the innermost target — which is what a drop over overlapping \
         targets must resolve to"
    );
}

/// A translucent tag is found without blocking what is behind it.
///
/// `Translucent` must appear in the hit path AND let lower siblings be tested —
/// two facts the single `bool` a `hit_test` returns cannot both carry, since
/// the bridge reads it as `add_self` *and* `blocks_below`. Returning `true`
/// makes a translucent node opaque to its siblings; the entry has to be
/// registered separately.
///
/// Both siblings are TAGGED so the oracle names them. A first draft asserted
/// only `path().len() > 1`, which the Stack and the root satisfy on their own —
/// it passed against the blocking bug it was written to catch.
///
/// The translucent node's own child deliberately misses (a bare `SizedBox`),
/// so the translucent-miss arm is the one under test.
#[test]
fn a_translucent_tag_is_found_without_blocking_what_is_behind_it() {
    use flui_view::ViewExt;

    let laid = lay_out(
        flui_widgets::Stack::new(vec![
            // Behind: claims hits on its own.
            MetaData::new(Slot("behind"))
                .behavior(HitTestBehavior::Opaque)
                .child(SizedBox::new(40.0, 40.0))
                .boxed(),
            // In front: translucent, over a child that misses.
            MetaData::new(Slot("overlay"))
                .behavior(HitTestBehavior::Translucent)
                .child(SizedBox::new(40.0, 40.0))
                .boxed(),
        ]),
        tight(40.0, 40.0),
    );

    let hit = laid.hit_test_pointer(offset(20.0, 20.0));
    let found: Vec<&Slot> = hit
        .path()
        .iter()
        .filter_map(|entry| entry.metadata_as::<Slot>())
        .collect();

    assert_eq!(
        found,
        vec![&Slot("overlay"), &Slot("behind")],
        "a translucent tag must appear in the path AND leave the sibling \
         behind it reachable; finding only the overlay means it swallowed the \
         hit, which is opaque behaviour"
    );
}
