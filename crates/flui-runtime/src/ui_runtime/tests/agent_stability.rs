//! What the actionability check remembers decides `stable` (R11): bounds
//! compare only once a frame has run since they were read, or while nothing is
//! pending that could still move the element.

use flui_protocol::{ElementId, ElementRef, LogicalRect, WindowId};

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

    for number in 3..(3 + 64) {
        let _ = stability.observe(element(number), revision, true, rect(0.0));
    }
    assert_eq!(
        stability.observe(element(1), revision, true, rect(1.0)),
        None,
        "a full table starts again rather than grow"
    );
}
