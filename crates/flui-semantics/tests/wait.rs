//! Wait conditions over the published tree (ADR-0238 §7, R18), through the public API: what
//! holds, what does not, and what the evaluation saw while deciding.

use flui_protocol::{Condition, Expectation, Locator, Role, StateFilter, TextMatch};
use flui_semantics::{SemanticsRole, Toggled, WaitFacts, counts_frames, waits_on_events};

use crate::fixture::{Fixture, placement};

const IDLE: WaitFacts = WaitFacts {
    frames: 0,
    settled: false,
};

/// A window with a "Save" button, a "Notify" check box that is on, and a disabled "Later"
/// button.
fn shown() -> Fixture {
    let mut f = Fixture::new();
    f.add(None, 1, (0.0, 0.0, 400.0, 300.0), |c| c.set_label("App"));
    f.add(Some(1), 2, (0.0, 0.0, 50.0, 20.0), |c| {
        c.set_role(SemanticsRole::Button);
        c.set_label("Save");
    });
    f.add(Some(1), 3, (0.0, 30.0, 50.0, 20.0), |c| {
        c.set_role(SemanticsRole::CheckBox);
        c.set_label("Notify");
        c.set_toggled(Some(Toggled::On));
    });
    f.add(Some(1), 4, (0.0, 60.0, 50.0, 20.0), |c| {
        c.set_role(SemanticsRole::Button);
        c.set_label("Later");
        c.set_enabled(Some(false));
    });
    f.flush();
    f
}

fn button(name: &str) -> Locator {
    Locator::role(Role::Button).name(name)
}

#[test]
fn a_condition_holds_or_not_against_the_published_tree() {
    let f = shown();
    let notify_on = states(|state| state.checked = Some(flui_protocol::Checked::True));
    let table = [
        (
            "appears: present",
            Condition::Appears(button("Save")),
            IDLE,
            true,
        ),
        (
            "appears: absent",
            Condition::Appears(button("Missing")),
            IDLE,
            false,
        ),
        (
            "gone: absent",
            Condition::Gone(button("Missing")),
            IDLE,
            true,
        ),
        (
            "gone: present",
            Condition::Gone(button("Save")),
            IDLE,
            false,
        ),
        (
            "name matches",
            Condition::Matches {
                locator: button("Save"),
                expect: Expectation::Name(TextMatch::contains("av")),
            },
            IDLE,
            true,
        ),
        (
            "name differs",
            Condition::Matches {
                locator: button("Save"),
                expect: Expectation::Name(TextMatch::exact("Saved")),
            },
            IDLE,
            false,
        ),
        (
            "state holds",
            Condition::Matches {
                locator: Locator::role(Role::CheckBox),
                expect: Expectation::State(notify_on),
            },
            IDLE,
            true,
        ),
        (
            "disabled state",
            Condition::Matches {
                locator: button("Later"),
                expect: Expectation::State(states(|state| state.disabled = Some(true))),
            },
            IDLE,
            true,
        ),
        (
            "count of buttons",
            Condition::Matches {
                locator: Locator::role(Role::Button),
                expect: Expectation::Count(2),
            },
            IDLE,
            true,
        ),
        (
            "count of none",
            Condition::Matches {
                locator: button("Missing"),
                expect: Expectation::Count(0),
            },
            IDLE,
            true,
        ),
        (
            "an expectation on no element does not hold",
            Condition::Matches {
                locator: button("Missing"),
                expect: Expectation::Name(TextMatch::contains("")),
            },
            IDLE,
            false,
        ),
        (
            "every matched element must meet it",
            Condition::Matches {
                locator: Locator::role(Role::Button),
                expect: Expectation::Name(TextMatch::exact("Save")),
            },
            IDLE,
            false,
        ),
        (
            "not focused",
            Condition::Matches {
                locator: button("Save"),
                expect: Expectation::Focused,
            },
            IDLE,
            false,
        ),
        (
            "settled",
            Condition::Settled,
            WaitFacts {
                settled: true,
                ..IDLE
            },
            true,
        ),
        ("unsettled", Condition::Settled, IDLE, false),
        (
            "frames reached",
            Condition::Frames(3),
            WaitFacts { frames: 3, ..IDLE },
            true,
        ),
        (
            "frames short",
            Condition::Frames(3),
            WaitFacts { frames: 2, ..IDLE },
            false,
        ),
        (
            "all needs every part",
            Condition::All(vec![
                Condition::Appears(button("Save")),
                Condition::Frames(1),
            ]),
            IDLE,
            false,
        ),
        (
            "any needs one part",
            Condition::Any(vec![
                Condition::Appears(button("Save")),
                Condition::Frames(1),
            ]),
            IDLE,
            true,
        ),
        (
            "an event is not seen here",
            Condition::Event(flui_protocol::EventFilter::new(
                flui_protocol::EventKind::Frame,
            )),
            WaitFacts {
                frames: 9,
                settled: true,
            },
            false,
        ),
    ];
    for (row, condition, facts, expected) in table {
        let evaluation = f
            .owner
            .evaluate_wait(&condition, facts, placement())
            .unwrap_or_else(|error| panic!("{row}: {error}"));
        assert_eq!(evaluation.holds, expected, "{row}");
    }
}

#[test]
fn a_window_that_has_drawn_nothing_makes_nothing_disappear() {
    let f = Fixture::new();
    for (row, condition, expected) in [
        ("appears", Condition::Appears(button("Save")), false),
        ("gone", Condition::Gone(button("Save")), false),
        (
            "count of none",
            Condition::Matches {
                locator: button("Save"),
                expect: Expectation::Count(0),
            },
            false,
        ),
        ("frames", Condition::Frames(0), true),
    ] {
        let evaluation = f
            .owner
            .evaluate_wait(&condition, IDLE, placement())
            .unwrap_or_else(|error| panic!("{row}: {error}"));
        assert_eq!(evaluation.holds, expected, "{row}");
        assert_eq!(evaluation.observation.count, 0, "{row}");
    }
}

#[test]
fn the_observation_names_the_revision_and_the_elements_matched() {
    let f = shown();
    let evaluation = f
        .owner
        .evaluate_wait(
            &Condition::All(vec![
                Condition::Appears(Locator::role(Role::Button)),
                Condition::Appears(Locator::role(Role::CheckBox)),
            ]),
            IDLE,
            placement(),
        )
        .expect("a flushed tree evaluates");
    assert!(evaluation.holds);
    assert_eq!(evaluation.observation.revision, f.owner.revision());
    assert_eq!(evaluation.observation.count, 3);
    let names: Vec<_> = evaluation
        .observation
        .matched
        .iter()
        .filter_map(|node| node.name.as_deref())
        .collect();
    assert_eq!(names, ["Save", "Later", "Notify"]);
}

#[test]
fn a_composite_reports_what_it_needs_from_the_runtime() {
    let frames = Condition::Frames(2);
    let event = Condition::Event(flui_protocol::EventFilter::new(
        flui_protocol::EventKind::Input,
    ));
    let nested = Condition::All(vec![
        Condition::Settled,
        Condition::Any(vec![frames.clone()]),
    ]);
    assert!(counts_frames(&frames) && counts_frames(&nested));
    assert!(!counts_frames(&Condition::Settled));
    assert!(waits_on_events(&event));
    assert!(waits_on_events(&Condition::Any(vec![event])));
    assert!(!waits_on_events(&nested));
}

/// A state filter with `set` applied (the filter is non-exhaustive).
fn states(set: impl FnOnce(&mut StateFilter)) -> StateFilter {
    let mut state = StateFilter::default();
    set(&mut state);
    state
}
