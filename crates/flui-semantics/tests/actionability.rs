//! The facts that decide whether input can reach an element (ADR-0238, R11),
//! through the public API: attached, visible, stable, enabled and receiving,
//! and the render-to-semantics mapping behind the last.

use flui_foundation::RenderId;
use flui_protocol::{Actionability, ActionabilityCheck, ElementRef, LogicalPoint, LogicalRect};
use flui_semantics::SemanticsRole;

use crate::fixture::{Fixture, element, element_of, placement, render_id};

/// A 400 × 300 window. Index → what it is:
///
/// ```text
///  1 window "Mail"
///   2 button "Plain" (10,10 80×30)
///   3 button "Edge", half outside the window (380,10 60×30)
///   4 button "Beyond", outside the window (500,10 50×30)
///   5 button "Empty", no size (10,60 0×0)
///   6 group "Locked" (disabled) → 7 button "Child" (enabled itself)
///   8 button "Hidden" (hidden)
///   9 button "Covered" (10,200 100×40), over which sits 10 panel "Banner"
///  11 button "Card" (200,200 100×50) → 12 label "Title"
/// ```
fn window() -> Fixture {
    let mut f = Fixture::new();
    f.add(None, 1, (0.0, 0.0, 400.0, 300.0), |c| c.set_label("Mail"));
    let button = |f: &mut Fixture, parent, index, label: &'static str, rect| {
        f.add(Some(parent), index, rect, |c| {
            c.set_role(SemanticsRole::Button);
            c.set_label(label);
        });
    };
    button(&mut f, 1, 2, "Plain", (10.0, 10.0, 80.0, 30.0));
    button(&mut f, 1, 3, "Edge", (380.0, 10.0, 60.0, 30.0));
    button(&mut f, 1, 4, "Beyond", (500.0, 10.0, 50.0, 30.0));
    button(&mut f, 1, 5, "Empty", (10.0, 60.0, 0.0, 0.0));
    f.add(Some(1), 6, (10.0, 100.0, 200.0, 100.0), |c| {
        c.set_role(SemanticsRole::ListBox);
        c.set_label("Locked");
        c.set_enabled(Some(false));
    });
    button(&mut f, 6, 7, "Child", (20.0, 110.0, 50.0, 20.0));
    f.add(Some(1), 8, (120.0, 10.0, 50.0, 30.0), |c| {
        c.set_role(SemanticsRole::Button);
        c.set_label("Hidden");
        c.set_hidden(true);
    });
    button(&mut f, 1, 9, "Covered", (10.0, 200.0, 100.0, 40.0));
    f.add(Some(1), 10, (10.0, 200.0, 100.0, 40.0), |c| {
        c.set_label("Banner");
    });
    f.add(Some(1), 11, (200.0, 200.0, 100.0, 50.0), |c| {
        c.set_role(SemanticsRole::Button);
        c.set_label("Card");
    });
    f.add(Some(11), 12, (210.0, 210.0, 50.0, 10.0), |c| {
        c.set_label("Title");
    });
    f.flush();
    f
}

fn rect(left: f64, top: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(left, top, width, height)
}

/// The facts about `target`, as the agent would assemble them: where the
/// pointer lands, then what a hit-test there finds (`hit`, most specific first).
fn facts(
    f: &Fixture,
    target: ElementRef,
    previous: Option<LogicalRect>,
    hit: &[RenderId],
) -> Actionability {
    let point = f
        .owner
        .actionability_point(target, None, placement())
        .expect("the tree reads");
    f.owner
        .actionability_wire(target, point, previous, hit, placement())
        .expect("the tree reads")
}

fn ids(indices: &[u32]) -> Vec<RenderId> {
    indices.iter().map(|&index| render_id(index, 5)).collect()
}

/// One element per row: the first check that fails, in the driver's order.
#[test]
fn the_first_failed_check_names_why_an_element_cannot_be_acted_on() {
    let f = window();
    let steady = Some(rect(10.0, 10.0, 80.0, 30.0));
    type Row = (
        &'static str,
        ElementRef,
        Option<LogicalRect>,
        Vec<RenderId>,
        Option<ActionabilityCheck>,
    );
    let table: Vec<Row> = vec![
        ("actionable", element(2), steady, ids(&[2, 1]), None),
        (
            "not in the tree",
            element(50),
            steady,
            ids(&[2]),
            Some(ActionabilityCheck::Attached),
        ),
        (
            "outside the window",
            element(4),
            None,
            ids(&[1]),
            Some(ActionabilityCheck::Visible),
        ),
        (
            "no size",
            element(5),
            None,
            ids(&[1]),
            Some(ActionabilityCheck::Visible),
        ),
        (
            "hidden",
            element(8),
            None,
            ids(&[1]),
            Some(ActionabilityCheck::Visible),
        ),
        (
            "no earlier reading to compare",
            element(2),
            None,
            ids(&[2]),
            Some(ActionabilityCheck::Stable),
        ),
        (
            "moved since the earlier reading",
            element(2),
            Some(rect(10.0, 14.0, 80.0, 30.0)),
            ids(&[2]),
            Some(ActionabilityCheck::Stable),
        ),
        (
            "a disabled ancestor",
            element(7),
            Some(rect(20.0, 110.0, 50.0, 20.0)),
            ids(&[7, 6]),
            Some(ActionabilityCheck::Enabled),
        ),
        (
            "covered by a node over it",
            element(9),
            Some(rect(10.0, 200.0, 100.0, 40.0)),
            ids(&[10, 1]),
            Some(ActionabilityCheck::Receives),
        ),
        (
            "nothing hit at all",
            element(2),
            steady,
            Vec::new(),
            Some(ActionabilityCheck::Receives),
        ),
    ];
    for (row, target, previous, hit, expected) in table {
        let found = facts(&f, target, previous, &hit);
        assert_eq!(found.first_failed(), expected, "{row}: {found:?}");
    }
}

#[test]
fn what_covers_the_point_is_named_by_its_outline() {
    let f = window();
    let covered = facts(&f, element(9), None, &ids(&[10, 1]));
    assert!(!covered.receives);
    let line = covered.covered_by.expect("the banner covers it");
    assert!(line.starts_with("- "), "{line}");
    assert!(line.contains("\"Banner\""), "{line}");
    assert!(!line.contains('\n'), "one line: {line:?}");

    // Nothing the tree shows was hit: nothing to name.
    let nothing = facts(&f, element(9), None, &ids(&[40, 41]));
    assert!(!nothing.receives);
    assert_eq!(nothing.covered_by, None);
}

/// The pointer reaches the first render object of the path that the tree
/// shows: a render object with no node of its own passes it up the path, and a
/// node below the element is the element.
#[test]
fn the_hit_maps_to_the_nearest_node_the_tree_shows() {
    let f = window();
    let table: Vec<(&str, ElementRef, Vec<RenderId>, bool)> = vec![
        (
            "the element's own render object",
            element(11),
            ids(&[11, 1]),
            true,
        ),
        ("a node below it", element(11), ids(&[12, 11, 1]), true),
        (
            "a render object with no node, then it",
            element(11),
            ids(&[30, 31, 11, 1]),
            true,
        ),
        ("an ancestor of it", element(12), ids(&[11, 1]), false),
        ("the window behind", element(11), ids(&[30, 1]), false),
        ("a sibling", element(11), ids(&[2, 1]), false),
    ];
    for (row, target, hit, receives) in table {
        let found = facts(&f, target, Some(rect_of(target)), &hit);
        assert_eq!(found.receives, receives, "{row}: {found:?}");
    }
}

fn rect_of(target: ElementRef) -> LogicalRect {
    if target == element(11) {
        rect(200.0, 200.0, 100.0, 50.0)
    } else {
        rect(210.0, 210.0, 50.0, 10.0)
    }
}

#[test]
fn the_pointer_aims_at_the_centre_of_what_the_window_shows() {
    let f = window();
    let at = |target, point| {
        f.owner
            .actionability_point(target, point, placement())
            .expect("the tree reads")
    };
    assert_eq!(at(element(2), None), Some(LogicalPoint::new(50.0, 25.0)));
    // Half outside: the middle of the half that is inside.
    assert_eq!(at(element(3), None), Some(LogicalPoint::new(390.0, 25.0)));
    // A point the caller chose is the point used.
    assert_eq!(
        at(element(2), Some(LogicalPoint::new(12.0, 14.0))),
        Some(LogicalPoint::new(12.0, 14.0))
    );
    // Nowhere to aim.
    assert_eq!(at(element(4), None), None);
    assert_eq!(at(element(5), None), None);
    assert_eq!(at(element(8), None), None);
    assert_eq!(at(element(50), None), None);

    let found = facts(
        &f,
        element(3),
        Some(rect(380.0, 10.0, 60.0, 30.0)),
        &ids(&[3]),
    );
    assert_eq!(found.point, Some(LogicalPoint::new(390.0, 25.0)));
    assert_eq!(
        found.rect,
        Some(rect(380.0, 10.0, 60.0, 30.0)),
        "the bounds as published"
    );
    assert!(found.visible && found.stable && found.enabled && found.receives);
}

#[test]
fn a_hidden_element_is_attached_and_an_absent_one_is_not() {
    let f = window();
    let hidden = facts(&f, element(8), None, &[]);
    assert!(hidden.attached && !hidden.visible);
    let absent = facts(&f, element(50), None, &[]);
    assert!(!absent.attached && !absent.visible && !absent.stable && !absent.enabled);
    assert_eq!(absent.rect, None);
}

#[test]
fn an_element_of_another_window_is_not_attached() {
    let f = window();
    let other = ElementRef::new(
        flui_protocol::WindowId::from_u64(2).expect("non-zero"),
        element(2).element,
    );
    assert!(!facts(&f, other, None, &ids(&[2])).attached);
}

#[test]
fn the_facts_carry_the_revision_they_were_read_on() {
    let f = window();
    assert_eq!(
        facts(&f, element(2), None, &[]).revision,
        f.owner.revision()
    );
}

/// A handle names the element of its generation: when the slot is reused the
/// old handle is not attached to the new element.
#[test]
fn a_handle_from_before_a_replacement_is_not_attached() {
    let mut f = window();
    let old = element(2);
    assert!(facts(&f, old, None, &[]).attached);

    let replaced = f.id_of(2);
    f.owner.remove(replaced);
    f.add_generation(Some(1), 2, 6, (10.0, 10.0, 80.0, 30.0), |c| {
        c.set_role(SemanticsRole::Button);
        c.set_label("Plain");
    });
    f.flush();
    assert!(!facts(&f, old, None, &[]).attached, "the old handle");
    let new = element_of(render_id(2, 6));
    assert!(facts(&f, new, None, &[]).attached, "the replacement");
}
