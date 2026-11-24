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
}

/// A minimum that is not a finite positive extent is refused, and one exactly
/// met admits the region, and a region with a non-finite extent is not.
#[test]
fn a_minimum_target_is_a_finite_positive_extent() {
    for refused in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(MinimumTarget::try_new(refused).is_none(), "{refused}");
    }
    let minimum = MinimumTarget::try_new(24.0).expect("24 px is a minimum");
    assert!(minimum.admits(24.0, 24.0));
    assert!(!minimum.admits(23.9, 24.0));
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
