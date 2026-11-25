//! The accessibility rules over a published tree, through the public API:
//! an interactive element with no name, and one whose hit region, inside the
//! window and the clips that do not scroll, is under the minimum target.

use std::rc::Rc;

use flui_foundation::geometry::AxisDirection;
use flui_protocol::audit::AuditRule;
use flui_semantics::{
    MinimumTarget, SemanticsAction, SemanticsConfiguration, SemanticsRole, Toggled,
};

use crate::fixture::{Fixture, element, placement};

/// A button that does something when invoked.
fn button(c: &mut SemanticsConfiguration, label: Option<&'static str>) {
    c.set_role(SemanticsRole::Button);
    if let Some(label) = label {
        c.set_label(label);
    }
    c.add_action(SemanticsAction::Click, Rc::new(|_| {}));
}

/// Marks the node at `index` as clipping its children.
fn clipping(f: &mut Fixture, index: u32) {
    let id = f.id_of(index);
    f.owner
        .get_mut(id)
        .expect("the fixture has the clipping node")
        .set_clips_children(true);
}

/// A 400 × 300 window. Index → what it is:
///
/// ```text
///  1 window "Settings"
///   2 button, no name (10,10 60×60)
///   3 button "Save" (100,10 60×60)
///   4 button "Tiny" (10,100 20×20)
///   5 button "Roomy" (100,100 48×48)
///   6 label "Caption" (200,100 10×10): not interactive
///   7 vertical list "List" (10,200 200×50), clipping its children →
///     8 button "Half shown" (10,226 100×48): half scrolled out, 48 px once
///       revealed
///     11 button "Thin half shown" (110,235 100×30): half scrolled out, 30 px
///        once revealed
///     12 button "Wide" (180,205 100×48): partly scrolled out, and cut to
///        30 px across by the list's side, which does not scroll
///   9 button "Hidden", no name (300,10 5×5), hidden
///  10 button "Beyond" (500,10 10×10), outside the window
///  13 frame "Frame" (250,200 100×14), clipping its children, not scrolling →
///     14 button "Cut" (250,200 100×60): cut to 14 px
///  15 horizontal list "Row" (220,100 100×50), clipping its children →
///     16 button "Half shown across" (296,100 48×50): half scrolled out, 48 px
///        once revealed
///     17 button "Thin across" (300,100 30×50): partly scrolled out, 30 px
///        once revealed
/// ```
fn window() -> Fixture {
    let mut f = Fixture::new();
    f.add(None, 1, (0.0, 0.0, 400.0, 300.0), |c| {
        c.set_label("Settings");
    });
    f.add(Some(1), 2, (10.0, 10.0, 60.0, 60.0), |c| button(c, None));
    f.add(Some(1), 3, (100.0, 10.0, 60.0, 60.0), |c| {
        button(c, Some("Save"));
    });
    f.add(Some(1), 4, (10.0, 100.0, 20.0, 20.0), |c| {
        button(c, Some("Tiny"));
    });
    f.add(Some(1), 5, (100.0, 100.0, 48.0, 48.0), |c| {
        button(c, Some("Roomy"));
    });
    f.add(Some(1), 6, (200.0, 100.0, 10.0, 10.0), |c| {
        c.set_label("Caption");
    });
    f.add(Some(1), 7, (10.0, 200.0, 200.0, 50.0), |c| {
        c.set_role(SemanticsRole::ScrollView);
        c.set_label("List");
        c.set_scroll_position(0.0);
    });
    clipping(&mut f, 7);
    f.add(Some(7), 8, (10.0, 226.0, 100.0, 48.0), |c| {
        button(c, Some("Half shown"));
    });
    f.add(Some(7), 11, (110.0, 235.0, 100.0, 30.0), |c| {
        button(c, Some("Thin half shown"));
    });
    f.add(Some(7), 12, (180.0, 205.0, 100.0, 48.0), |c| {
        button(c, Some("Wide"));
    });
    f.add(Some(1), 9, (300.0, 10.0, 5.0, 5.0), |c| {
        button(c, None);
        c.set_hidden(true);
    });
    f.add(Some(1), 10, (500.0, 10.0, 10.0, 10.0), |c| {
        button(c, Some("Beyond"));
    });
    f.add(Some(1), 13, (250.0, 200.0, 100.0, 14.0), |c| {
        c.set_label("Frame");
    });
    clipping(&mut f, 13);
    f.add(Some(13), 14, (250.0, 200.0, 100.0, 60.0), |c| {
        button(c, Some("Cut"));
    });
    f.add(Some(1), 15, (220.0, 100.0, 100.0, 50.0), |c| {
        c.set_role(SemanticsRole::ScrollView);
        c.set_label("Row");
        c.set_scroll_axis_direction(AxisDirection::LeftToRight);
        c.set_scroll_position(0.0);
    });
    clipping(&mut f, 15);
    f.add(Some(15), 16, (296.0, 100.0, 48.0, 50.0), |c| {
        button(c, Some("Half shown across"));
    });
    f.add(Some(15), 17, (300.0, 100.0, 30.0, 50.0), |c| {
        button(c, Some("Thin across"));
    });
    f.flush();
    f
}

/// Each rule names exactly the elements that break it, with a detail a person
/// can act on: an unnamed button and not its named twin; an undersized target
/// and not its corrected twin. A row partly scrolled out of a list, down or
/// across, is judged by its size once revealed and reported only when that is
/// under the minimum. A target a clip that does not scroll cuts, or a list's
/// side cuts across its axis, is judged by what that clip leaves. A hidden
/// element, a label and an element outside the window are not judged.
#[test]
fn each_rule_finds_the_elements_that_break_it_and_spares_their_twins() {
    let f = window();
    let minimum = MinimumTarget::try_new(44.0).expect("44 px is a minimum");
    let unnamed = f
        .owner
        .audit_unnamed(placement())
        .expect("the published tree reads");
    let undersized = f
        .owner
        .audit_target_size(minimum, placement())
        .expect("the published tree reads");

    type Row = (
        &'static str,
        Vec<flui_protocol::audit::AuditFinding>,
        AuditRule,
        Vec<u32>,
        &'static str,
    );
    let table: Vec<Row> = vec![
        (
            "an unnamed button, not its named twin",
            unnamed,
            AuditRule::Unnamed,
            vec![2],
            "no accessible name",
        ),
        (
            "an undersized target, a thin row scrolled partly out of each list, a row the \
             vertical list cuts across and a target a fixed clip cuts; not the roomy twin nor \
             the 48 px rows scrolled half out",
            undersized,
            AuditRule::TargetSize,
            vec![4, 11, 12, 14, 17],
            "under the platform's 44 px minimum target",
        ),
    ];
    for (name, findings, rule, expected, detail) in table {
        let nodes: Vec<_> = findings.iter().map(|finding| finding.node).collect();
        let wanted: Vec<_> = expected.iter().map(|&index| element(index)).collect();
        assert_eq!(nodes, wanted, "{name}: {findings:#?}");
        for finding in &findings {
            assert_eq!(finding.rule, rule, "{name}");
            assert!(
                finding.detail.contains(detail),
                "{name}: {}",
                finding.detail
            );
        }
    }
}

/// A finding gives the extent it judged: a row scrolled partly out its whole
/// size along the scroll axis, with no mention of a clip; a target a clip that
/// does not scroll cuts, the part that clip leaves and the bounds it was cut
/// from; a row a list's side cuts, the width the side leaves and its whole
/// height.
#[test]
fn a_target_size_finding_names_the_hit_region_inside_the_clip() {
    let f = window();
    let minimum = MinimumTarget::try_new(44.0).expect("44 px is a minimum");
    let findings = f
        .owner
        .audit_target_size(minimum, placement())
        .expect("the published tree reads");
    let detail = |index| {
        findings
            .iter()
            .find(|finding| finding.node == element(index))
            .map_or_else(
                || panic!("no finding for {index}: {findings:#?}"),
                |finding| finding.detail.clone(),
            )
    };
    let under = "under the platform's 44 px minimum target";
    for (index, region) in [
        (4, "20 × 20 logical px"),
        (11, "100 × 30 logical px"),
        (12, "30 × 48 logical px, clipped from its 100 × 48 bounds"),
        (14, "100 × 14 logical px, clipped from its 100 × 60 bounds"),
        (17, "30 × 50 logical px"),
    ] {
        assert_eq!(
            detail(index),
            format!("this button's hit region is {region}, {under}"),
            "{index}"
        );
    }
}

/// A vertical scrollable at `rect`, clipping its children.
fn vertical_list(f: &mut Fixture, parent: u32, index: u32, rect: (f64, f64, f64, f64)) {
    f.add(Some(parent), index, rect, |c| {
        c.set_role(SemanticsRole::ScrollView);
        c.set_label("List");
        c.set_scroll_position(0.0);
    });
    clipping(f, index);
}

/// An 800 × 600 window. Index → what it is:
///
/// ```text
///  1 window
///   2 vertical list (10,10 200×30) →
///     3 button "Tall" (10,10 100×200): longer than the 30 px the list shows
///   4 vertical list (300,10 200×30) →
///     5 vertical list (300,10 200×300) →
///       6 button "Nested" (300,10 100×100): the outer list shows 30 px of it
///   7 frame (550,0 200×30), clipping its children, not scrolling →
///     8 vertical list (550,0 200×300) →
///       9 button "Framed" (550,20 100×100): the frame leaves 30 px of the list
///  10 button "Edge" (770,100 60×60): the window's right edge cuts it to 30 px
///  11 button "Bottom" (100,570 60×60): the window's bottom edge cuts it to 30 px
/// ```
fn revealed() -> Fixture {
    let mut f = Fixture::new();
    f.add(None, 1, (0.0, 0.0, 800.0, 600.0), |c| {
        c.set_label("Window");
    });
    vertical_list(&mut f, 1, 2, (10.0, 10.0, 200.0, 30.0));
    f.add(Some(2), 3, (10.0, 10.0, 100.0, 200.0), |c| {
        button(c, Some("Tall"));
    });
    vertical_list(&mut f, 1, 4, (300.0, 10.0, 200.0, 30.0));
    vertical_list(&mut f, 4, 5, (300.0, 10.0, 200.0, 300.0));
    f.add(Some(5), 6, (300.0, 10.0, 100.0, 100.0), |c| {
        button(c, Some("Nested"));
    });
    f.add(Some(1), 7, (550.0, 0.0, 200.0, 30.0), |c| {
        c.set_label("Frame");
    });
    clipping(&mut f, 7);
    vertical_list(&mut f, 7, 8, (550.0, 0.0, 200.0, 300.0));
    f.add(Some(8), 9, (550.0, 20.0, 100.0, 100.0), |c| {
        button(c, Some("Framed"));
    });
    f.add(Some(1), 10, (770.0, 100.0, 60.0, 60.0), |c| {
        button(c, Some("Edge"));
    });
    f.add(Some(1), 11, (100.0, 570.0, 60.0, 60.0), |c| {
        button(c, Some("Bottom"));
    });
    f.flush();
    f
}

/// Along the axis a scrollable scrolls, a target is judged by the extent it
/// has once revealed, which is at most what that scrollable shows: a target
/// longer than its list, one inside a list inside a shorter list, and one in a
/// list a fixed clip cuts are each judged by the span the outermost of those
/// leaves, not by their own length. The window's edge cuts like a fixed clip.
#[test]
fn a_revealed_target_is_judged_by_what_its_scrollables_show() {
    let f = revealed();
    let minimum = MinimumTarget::try_new(44.0).expect("44 px is a minimum");
    let findings = f
        .owner
        .audit_target_size(minimum, placement())
        .expect("the published tree reads");
    let under = "under the platform's 44 px minimum target";
    let expected = [
        (3, "100 × 30 logical px, clipped from its 100 × 200 bounds"),
        (6, "100 × 30 logical px, clipped from its 100 × 100 bounds"),
        (9, "100 × 30 logical px, clipped from its 100 × 100 bounds"),
        (10, "30 × 60 logical px, clipped from its 60 × 60 bounds"),
        (11, "60 × 30 logical px, clipped from its 60 × 60 bounds"),
    ];
    let found: Vec<_> = findings
        .iter()
        .map(|finding| (finding.node, finding.detail.clone()))
        .collect();
    let wanted: Vec<_> = expected
        .iter()
        .map(|&(index, region)| {
            (
                element(index),
                format!("this button's hit region is {region}, {under}"),
            )
        })
        .collect();
    assert_eq!(found, wanted);
}

/// An element is interactive by any action that operates it, not only
/// `invoke`: an unnamed control that advertises only `toggle`, `set_value`,
/// `select`, `expand` or `collapse` is reported, and one that only takes focus
/// is not.
#[test]
fn each_action_that_operates_a_control_makes_it_interactive() {
    let noop = || Rc::new(|_| {});
    let mut f = Fixture::new();
    f.add(None, 1, (0.0, 0.0, 400.0, 300.0), |c| {
        c.set_label("Form");
    });
    f.add(Some(1), 2, (10.0, 10.0, 60.0, 60.0), |c| {
        c.set_role(SemanticsRole::CheckBox);
        c.set_toggled(Some(Toggled::Off));
    });
    f.add(Some(1), 3, (100.0, 10.0, 60.0, 60.0), |c| {
        c.set_role(SemanticsRole::TextInput);
        c.add_action(SemanticsAction::SetValue, noop());
    });
    f.add(Some(1), 4, (200.0, 10.0, 60.0, 60.0), |c| {
        c.set_role(SemanticsRole::Tab);
        c.set_selected(false);
    });
    f.add(Some(1), 5, (10.0, 100.0, 60.0, 60.0), |c| {
        c.set_role(SemanticsRole::TreeItem);
        c.set_expanded(false);
        c.add_action(SemanticsAction::Expand, noop());
    });
    f.add(Some(1), 6, (100.0, 100.0, 60.0, 60.0), |c| {
        c.set_role(SemanticsRole::TreeItem);
        c.set_expanded(true);
        c.add_action(SemanticsAction::Collapse, noop());
    });
    f.add(Some(1), 7, (200.0, 100.0, 60.0, 60.0), |c| {
        c.set_role(SemanticsRole::TextInput);
        c.set_read_only(true);
        c.add_action(SemanticsAction::Focus, noop());
    });
    f.flush();
    let findings = f
        .owner
        .audit_unnamed(placement())
        .expect("the published tree reads");
    let nodes: Vec<_> = findings.iter().map(|finding| finding.node).collect();
    let wanted: Vec<_> = [2, 3, 4, 5, 6].into_iter().map(element).collect();
    assert_eq!(nodes, wanted, "{findings:#?}");
    // A finding names the role in words: `check_box` reads as "check box".
    assert!(
        findings[0]
            .detail
            .starts_with("this check box has no accessible name"),
        "{}",
        findings[0].detail
    );
}

/// The nodes of a 3000 × 3000 window at `ratio` holding one named button per
/// `(left, top, width, height)`, numbered from 2 in order.
fn buttons_at(ratio: f64, rects: &[(f64, f64, f64, f64)]) -> Fixture {
    let mut f = Fixture::new();
    f.add(None, 1, (0.0, 0.0, 3000.0, 3000.0), |c| {
        c.set_label("Window");
    });
    for (index, &rect) in (2..).zip(rects) {
        f.add(Some(1), index, rect, |c| button(c, Some("Target")));
    }
    f.owner.set_device_pixel_ratio(ratio);
    f.flush();
    f
}

/// A target laid out at exactly the minimum meets it at every device ratio,
/// though the published tree scales its edges to device pixels and the audit
/// scales them back: at a ratio that is not a power of two the round trip
/// leaves the extent a few ulps under the minimum at some offsets. A target
/// clearly under the minimum is still reported at each of those ratios.
#[test]
fn a_target_at_the_minimum_meets_it_at_every_device_ratio() {
    let offsets = (0_u32..48).map(|step| {
        let step = f64::from(step);
        (1.0 + step * 61.0, 3.0 + step * 59.0)
    });
    for ratio in [1.1, 1.2, 1.25, 1.3, 1.4, 1.5, 1.6, 1.75, 2.625] {
        for size in [24.0, 44.0, 48.0] {
            let minimum = MinimumTarget::try_new(size).expect("a minimum");
            let at_minimum: Vec<_> = offsets
                .clone()
                .map(|(left, top)| (left, top, size, size))
                .collect();
            let met = buttons_at(ratio, &at_minimum)
                .owner
                .audit_target_size(minimum, placement())
                .expect("the published tree reads");
            assert!(met.is_empty(), "{size} px at {ratio}: {met:#?}");

            let under: Vec<_> = offsets
                .clone()
                .map(|(left, top)| (left, top, size - 0.1, size))
                .collect();
            let reported = buttons_at(ratio, &under)
                .owner
                .audit_target_size(minimum, placement())
                .expect("the published tree reads");
            assert_eq!(
                reported.len(),
                under.len(),
                "{} px of {size} at {ratio} is under the minimum: {reported:#?}",
                size - 0.1
            );
        }
    }
}

/// A minimum that is not a finite positive extent is refused, one met to
/// within half a hundredth of a pixel admits the region, and a region with a
/// non-finite extent is not.
#[test]
fn a_minimum_target_is_a_finite_positive_extent() {
    for refused in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(MinimumTarget::try_new(refused).is_none(), "{refused}");
    }
    let minimum = MinimumTarget::try_new(24.0).expect("24 px is a minimum");
    assert!(minimum.admits(24.0, 24.0));
    assert!(!minimum.admits(23.9, 24.0));
    // Within half a hundredth of the minimum is the minimum: round-off, never
    // a smaller target a device draws.
    assert!(minimum.admits(23.996, 23.996));
    assert!(!minimum.admits(23.994, 24.0));
    assert!(!minimum.admits(24.0, 23.994));
    for (width, height) in [
        (24.0, f64::NAN),
        (f64::NAN, 24.0),
        (f64::INFINITY, 24.0),
        (24.0, f64::INFINITY),
        (f64::INFINITY, f64::INFINITY),
    ] {
        assert!(
            !minimum.admits(width, height),
            "a {width} × {height} region is not admitted"
        );
    }
}
