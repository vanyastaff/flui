//! The accessibility rules over a published tree, through the public API:
//! an interactive element with no name, and one whose hit region inside its
//! clip is under the minimum target.

use std::rc::Rc;

use flui_protocol::audit::AuditRule;
use flui_semantics::{MinimumTarget, SemanticsAction, SemanticsConfiguration, SemanticsRole};

use crate::fixture::{Fixture, element, placement};

/// A button that does something when invoked.
fn button(c: &mut SemanticsConfiguration, label: Option<&'static str>) {
    c.set_role(SemanticsRole::Button);
    if let Some(label) = label {
        c.set_label(label);
    }
    c.add_action(SemanticsAction::Click, Rc::new(|_| {}));
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
///   7 scroll view "List" (10,200 200×50), clipping its children →
///     8 button "Half shown" (10,236 200×60): its hit region is the 14 px
///     strip inside the scroll view
///   9 button "Hidden", no name (300,10 5×5), hidden
///  10 button "Beyond" (500,10 10×10), outside the window
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
    let list = f.id_of(7);
    f.owner
        .get_mut(list)
        .expect("the fixture has the scroll view")
        .set_clips_children(true);
    f.add(Some(7), 8, (10.0, 236.0, 200.0, 60.0), |c| {
        button(c, Some("Half shown"));
    });
    f.add(Some(1), 9, (300.0, 10.0, 5.0, 5.0), |c| {
        button(c, None);
        c.set_hidden(true);
    });
    f.add(Some(1), 10, (500.0, 10.0, 10.0, 10.0), |c| {
        button(c, Some("Beyond"));
    });
    f.flush();
    f
}

/// Each rule names exactly the elements that break it, with a detail a person
/// can act on: an unnamed button and not its named twin; an undersized target
/// and not its corrected twin; a target drawn large but clipped to an
/// undersized hit region. A hidden element, a label and an element outside the
/// window are not judged.
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
            "an undersized target and a clipped one, not their roomy twin",
            undersized,
            AuditRule::TargetSize,
            vec![4, 8],
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

/// The clipped target's finding gives the hit region the scroll view leaves,
/// not its drawn bounds, and says it was clipped; the plain one gives its
/// bounds.
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
    assert_eq!(
        detail(4),
        "this button's hit region is 20 × 20 logical px, under the platform's 44 px minimum target"
    );
    assert_eq!(
        detail(8),
        "this button's hit region is 200 × 14 logical px, clipped from its 200 × 60 bounds, \
         under the platform's 44 px minimum target"
    );
}

/// A minimum that is not a finite positive extent is refused, and one exactly
/// met admits the region.
#[test]
fn a_minimum_target_is_a_finite_positive_extent() {
    for refused in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(MinimumTarget::try_new(refused).is_none(), "{refused}");
    }
    let minimum = MinimumTarget::try_new(24.0).expect("24 px is a minimum");
    assert!(minimum.admits(24.0, 24.0));
    assert!(!minimum.admits(23.9, 24.0));
    assert!(!minimum.admits(24.0, f64::NAN));
}
