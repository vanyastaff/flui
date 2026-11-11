//! `Region` and `DamageRegion` (ADR-0304 §5): up to eight disjoint rects, a
//! ninth merged into the cheapest pair, non-finite input escalated to `Full`.

use flui_foundation::geometry::Rect;
use flui_layer::{DamageRect, DamageRegion, FullReason, NonFinite, Region, ResyncReason};

const SURFACE: (u32, u32) = (1000, 1000);

/// The damage rect whose edges are exactly `l, t, r, b` (the covering adds a
/// one-pixel margin, taken back here).
fn rect(l: u32, t: u32, r: u32, b: u32) -> DamageRect {
    DamageRect::try_covering(
        Rect::from_ltrb(
            f64::from(l) + 1.0,
            f64::from(t) + 1.0,
            f64::from(r) - 1.0,
            f64::from(b) - 1.0,
        ),
        SURFACE,
    )
    .expect("finite")
    .expect("on the surface")
}

fn ltrb(rect: DamageRect) -> (u32, u32, u32, u32) {
    (rect.left(), rect.top(), rect.right(), rect.bottom())
}

/// Eight disjoint 10×10 squares on a row, 20 apart.
fn eight_squares() -> Region {
    let mut region = Region::new(rect(0, 0, 10, 10));
    for i in 1..8 {
        region.insert(rect(i * 20, 0, i * 20 + 10, 10));
    }
    region
}

pub(crate) fn a_ninth_rect_merges_the_cheapest_pair() {
    let mut region = eight_squares();
    assert_eq!(region.rects().len(), Region::MAX);
    // A ninth square right beside the last one: merging those two adds the
    // least area (a 2-pixel gap of 10 rows), less than any other pair.
    region.insert(rect(152, 0, 162, 10));
    assert_eq!(region.rects().len(), Region::MAX);
    assert!(
        region.rects().iter().any(|r| ltrb(*r) == (140, 0, 162, 10)),
        "the closest pair merged: {:?}",
        region.rects()
    );
    for i in 0..7 {
        assert!(
            region
                .rects()
                .iter()
                .any(|r| ltrb(*r) == (i * 20, 0, i * 20 + 10, 10)),
            "square {i} kept apart"
        );
    }
}

pub(crate) fn merged_rects_stay_pairwise_disjoint() {
    // A rect overlapping two of the squares merges with both, and the box it
    // grows into reaches a third, which merges too.
    let mut region = eight_squares();
    region.insert(rect(5, 5, 25, 30));
    region.insert(rect(0, 20, 45, 40));
    let rects = region.rects();
    for (i, a) in rects.iter().enumerate() {
        assert!(a.area() > 0, "no empty rect");
        for b in &rects[i + 1..] {
            assert!(!a.overlaps(*b), "{a:?} overlaps {b:?}");
        }
    }
    // Every inserted pixel is still covered.
    let covered = |x: u32, y: u32| {
        rects
            .iter()
            .any(|r| (r.left()..r.right()).contains(&x) && (r.top()..r.bottom()).contains(&y))
    };
    for (x, y) in [(5, 5), (24, 29), (0, 39), (44, 20), (145, 5)] {
        assert!(covered(x, y), "({x}, {y}) covered");
    }
    assert_eq!(
        region.area(),
        rects.iter().map(|r| r.area()).sum::<u64>(),
        "area is the sum of disjoint rects"
    );
    assert!(
        ltrb(region.bounds()) == (0, 0, 150, 40),
        "bounds {:?}",
        region.bounds()
    );
}

pub(crate) fn a_non_finite_rect_escalates_to_full() {
    for edge in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            DamageRect::try_covering(Rect::from_ltrb(0.0, 0.0, edge, 10.0), SURFACE),
            Err(NonFinite)
        );
    }
    assert_eq!(
        DamageRect::try_covering(Rect::from_ltrb(5.0, 5.0, 5.0, 10.0), SURFACE),
        Ok(None),
        "an empty rect is not an error"
    );
    assert_eq!(
        DamageRect::try_covering(Rect::from_ltrb(2000.0, 0.0, 2100.0, 10.0), SURFACE),
        Ok(None),
        "neither is one off the surface"
    );
}

pub(crate) fn full_keeps_its_first_reason() {
    let region = DamageRegion::Region(Region::new(rect(0, 0, 10, 10)));
    let first = DamageRegion::Full(FullReason::SurfaceChanged);
    let second = DamageRegion::Full(FullReason::Resync(ResyncReason::BaseMismatch));
    assert_eq!(first.union(second), first);
    assert_eq!(second.union(first), second);
    assert_eq!(region.union(first).union(second), first);
    assert_eq!(DamageRegion::Unchanged.union(second).union(first), second);
}
