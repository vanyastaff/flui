//! What the actionability check remembers decides `stable` (ADR-0238 §6): bounds
//! compare only once a frame has run since they were read, or while nothing is
//! pending that could still move the element.

use flui_protocol::{geometry::LogicalRect, tree::ElementId, tree::ElementRef, tree::WindowId};

use crate::epoch::TreeRevision;
use crate::ui_runtime::AgentStability;

fn element(number: u64) -> ElementRef {
    ElementRef::new(
        WindowId::from_u64(1).expect("non-zero"),
        ElementId::from_u64(number).expect("non-zero"),
    )
}

#[test]
fn bounds_compare_across_a_frame_or_an_idle_presentation_and_not_otherwise() {
    let before = TreeRevision::ZERO;
    let after = before.next();
    let rect = |x: f64| Some(LogicalRect::new(x, 0.0, 10.0, 10.0));
    let mut stability = AgentStability::default();

    assert_eq!(
        stability.observe(element(1), before, false, rect(1.0)),
        None,
        "first sight"
    );
    assert_eq!(
        stability.observe(element(1), before, false, rect(1.0)),
        None,
        "no frame since, and work pending: nothing to compare with"
    );
    assert_eq!(
        stability.observe(element(1), before, true, rect(2.0)),
        rect(1.0),
        "nothing pending: the earlier bounds are the last the element had"
    );
    assert_eq!(
        stability.observe(element(1), after, false, rect(2.0)),
        rect(2.0),
        "a frame ran since: compared even with work pending"
    );
    assert_eq!(
        stability.observe(element(1), after, false, None),
        None,
        "an element that left is forgotten"
    );
    assert_eq!(
        stability.observe(element(1), after, true, rect(2.0)),
        None,
        "...so it is a first sight again"
    );
}

/// A read at the same revision with work pending replays only a comparison made across a frame:
/// one made because the presentation was idle held only while nothing was pending, and work
/// pending now may move the element on the next frame.
#[test]
fn a_comparison_made_while_idle_is_not_replayed_once_work_is_pending() {
    let revision = TreeRevision::ZERO;
    let rect = |x: f64| Some(LogicalRect::new(x, 0.0, 10.0, 10.0));
    let mut stability = AgentStability::default();
    let _ = stability.observe(element(1), revision, false, rect(1.0));
    assert_eq!(
        stability.observe(element(1), revision, true, rect(1.0)),
        rect(1.0),
        "idle: compared with the last bounds"
    );
    assert_eq!(
        stability.observe(element(1), revision, false, rect(1.0)),
        None,
        "work pending at the same revision: the idle comparison no longer holds"
    );

    let after = revision.next();
    assert_eq!(
        stability.observe(element(1), after, false, rect(3.0)),
        rect(1.0),
        "a frame ran: compared across it"
    );
    assert_eq!(
        stability.observe(element(1), after, true, rect(3.0)),
        rect(3.0),
        "idle at that revision"
    );
    assert_eq!(
        stability.observe(element(1), after, false, rect(3.0)),
        rect(1.0),
        "work pending again: the comparison across the frame is replayed, not the idle one"
    );
}

#[test]
fn elements_are_remembered_apart_and_the_table_stays_bounded() {
    let revision = TreeRevision::ZERO;
    let rect = |x: f64| Some(LogicalRect::new(x, 0.0, 10.0, 10.0));
    let mut stability = AgentStability::default();
    assert_eq!(
        stability.observe(element(1), revision, true, rect(1.0)),
        None
    );
    assert_eq!(
        stability.observe(element(2), revision, true, rect(5.0)),
        None
    );
    assert_eq!(
        stability.observe(element(1), revision, true, rect(1.0)),
        rect(1.0)
    );
    assert_eq!(
        stability.observe(element(2), revision, true, rect(5.0)),
        rect(5.0)
    );

    // The table holds 192 elements: two for each of the 64 acts a presentation holds, and 64
    // more. Filled with elements 1 and 2 read longest ago, it still holds both.
    for number in 3..(3 + 190) {
        let _ = stability.observe(element(number), revision, true, rect(0.0));
    }
    assert_eq!(
        stability.observe(element(1), revision, true, rect(1.0)),
        rect(1.0),
        "a table of 192 holds every element read between two boundaries"
    );
    let _ = stability.observe(element(500), revision, true, rect(0.0));
    assert_eq!(
        stability.observe(element(1), revision, true, rect(1.0)),
        rect(1.0),
        "past the bound the element read longest ago is forgotten, not one read since"
    );
    assert_eq!(
        stability.observe(element(2), revision, true, rect(5.0)),
        None,
        "the element read longest ago is a first sight again"
    );
}
