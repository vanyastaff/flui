//! Locators resolved over a published tree (ADR-0238 §4, R6, R7, R9), through
//! the public API: every criterion, scoping, strictness, `pick`, Unicode case
//! folding, and what a failed resolution lists.

use flui_protocol::{
    Checked, ElementRef, LocateParams, Locator, Pick, Role, StateFilter, TextMatch,
};
use flui_semantics::{SemanticsRole, Toggled, WireLocateError};

use crate::fixture::{Fixture, element, placement, window};

/// A window with a toolbar, two lists, checkboxes, disclosure buttons, a text
/// field, names whose case folds beyond ASCII, a container the adapters lift
/// away and a hidden node. Index → what it is:
///
/// ```text
///  1 window "Mail"
///   2 button "Save" #save            3 button "Save as…" (hint)   4 link "Save"
///   5 list box "Inbox"
///     6 option "Alpha"   7 option "Beta" (selected)   8 option "Gamma" (disabled)
///     9 button "Delete" #delete
///   10 list box "Archive"
///     11 option "Alpha"  12 button "Delete" #delete
///   13 check box "Notify" (checked)  14 check box "Compact" (unchecked)
///   15 button "Details" (collapsed)  16 button "Options" (expanded)
///   17 text input "Subject" = "Hello World" (focused)
///   18 button "Straße"  19 button "ÉCOLE"
///   20 (unnamed container) → 21 button "Inner"
///   22 button "Ghost" (hidden)
///   23 list box "Locked" (disabled) → 24 button "Child" (enabled itself)
/// ```
fn mail() -> Fixture {
    let mut f = Fixture::new();
    let at = |index: u32| (f64::from(index) * 10.0, 0.0, 9.0, 9.0);
    f.add(None, 1, (0.0, 0.0, 400.0, 300.0), |c| c.set_label("Mail"));
    f.add(Some(1), 2, at(2), |c| {
        c.set_role(SemanticsRole::Button);
        c.set_label("Save");
        c.set_identifier("save");
    });
    f.add(Some(1), 3, at(3), |c| {
        c.set_role(SemanticsRole::Button);
        c.set_label("Save as…");
        c.set_hint("Saves a copy under a new name");
    });
    f.add(Some(1), 4, at(4), |c| {
        c.set_role(SemanticsRole::Link);
        c.set_label("Save");
    });
    f.add(Some(1), 5, at(5), |c| {
        c.set_role(SemanticsRole::ListBox);
        c.set_label("Inbox");
    });
    f.add(Some(5), 6, at(6), |c| {
        c.set_role(SemanticsRole::ListBoxOption);
        c.set_label("Alpha");
        c.set_selected(false);
    });
    f.add(Some(5), 7, at(7), |c| {
        c.set_role(SemanticsRole::ListBoxOption);
        c.set_label("Beta");
        c.set_selected(true);
    });
    f.add(Some(5), 8, at(8), |c| {
        c.set_role(SemanticsRole::ListBoxOption);
        c.set_label("Gamma");
        c.set_selected(false);
        c.set_enabled(Some(false));
    });
    f.add(Some(5), 9, at(9), |c| {
        c.set_role(SemanticsRole::Button);
        c.set_label("Delete");
        c.set_identifier("delete");
    });
    f.add(Some(1), 10, at(10), |c| {
        c.set_role(SemanticsRole::ListBox);
        c.set_label("Archive");
    });
    f.add(Some(10), 11, at(11), |c| {
        c.set_role(SemanticsRole::ListBoxOption);
        c.set_label("Alpha");
        c.set_selected(false);
    });
    f.add(Some(10), 12, at(12), |c| {
        c.set_role(SemanticsRole::Button);
        c.set_label("Delete");
        c.set_identifier("delete");
    });
    f.add(Some(1), 13, at(13), |c| {
        c.set_role(SemanticsRole::CheckBox);
        c.set_label("Notify");
        c.set_toggled(Some(Toggled::On));
    });
    f.add(Some(1), 14, at(14), |c| {
        c.set_role(SemanticsRole::CheckBox);
        c.set_label("Compact");
        c.set_toggled(Some(Toggled::Off));
    });
    f.add(Some(1), 15, at(15), |c| {
        c.set_role(SemanticsRole::Button);
        c.set_label("Details");
        c.set_expanded(false);
    });
    f.add(Some(1), 16, at(16), |c| {
        c.set_role(SemanticsRole::Button);
        c.set_label("Options");
        c.set_expanded(true);
    });
    f.add(Some(1), 17, at(17), |c| {
        c.set_role(SemanticsRole::TextInput);
        c.set_label("Subject");
        c.set_value("Hello World");
        c.set_focused(true);
    });
    f.add(Some(1), 18, at(18), |c| {
        c.set_role(SemanticsRole::Button);
        c.set_label("Straße");
    });
    f.add(Some(1), 19, at(19), |c| {
        c.set_role(SemanticsRole::Button);
        c.set_label("ÉCOLE");
    });
    f.add(Some(1), 20, at(20), |_| {});
    f.add(Some(20), 21, at(21), |c| {
        c.set_role(SemanticsRole::Button);
        c.set_label("Inner");
    });
    f.add(Some(1), 22, at(22), |c| {
        c.set_role(SemanticsRole::Button);
        c.set_label("Ghost");
        c.set_hidden(true);
    });
    f.add(Some(1), 23, at(23), |c| {
        c.set_role(SemanticsRole::ListBox);
        c.set_label("Locked");
        c.set_enabled(Some(false));
    });
    f.add(Some(23), 24, at(24), |c| {
        c.set_role(SemanticsRole::Button);
        c.set_label("Child");
    });
    f.flush();
    f
}

/// The fixture indices `locator` matches, in tree order.
fn located(f: &Fixture, locator: Locator) -> Vec<u32> {
    let matches = f
        .owner
        .locate_wire(&LocateParams::new(locator), placement())
        .expect("the fixture tree reads");
    matches
        .matches
        .iter()
        .map(|found| {
            (1..=24)
                .find(|index| element(*index) == found.element)
                .expect("a match is a fixture node")
        })
        .collect()
}

fn state(edit: impl FnOnce(&mut StateFilter)) -> StateFilter {
    let mut state = StateFilter::default();
    edit(&mut state);
    state
}

fn inbox() -> Locator {
    Locator::role(Role::List).name("Inbox")
}

/// One locator per row, over one tree: each criterion alone, with its case
/// and prefix forms, then scoping, then the window.
#[test]
fn a_locator_matches_what_it_describes_and_nothing_else() {
    let f = mail();
    let table: Vec<(&str, Locator, Vec<u32>)> = vec![
        (
            "role and name",
            Locator::role(Role::Button).name("Save"),
            vec![2],
        ),
        (
            "the same name under another role",
            Locator::role(Role::Link).name("Save"),
            vec![4],
        ),
        ("name alone", Locator::named("Save"), vec![2, 4]),
        (
            "name contains",
            Locator::default().name_matching(TextMatch::contains("Save")),
            vec![2, 3, 4],
        ),
        (
            "name prefix",
            Locator::default().name_matching(TextMatch::prefix("Save a")),
            vec![3],
        ),
        (
            "contains is case-sensitive by default",
            Locator::default().name_matching(TextMatch::contains("SAVE")),
            vec![],
        ),
        (
            "contains, ignoring case",
            Locator::default().name_matching(TextMatch::contains("SAVE").ignoring_case()),
            vec![2, 3, 4],
        ),
        (
            "folding reaches beyond ASCII: ß is SS",
            Locator::default().name_matching(TextMatch::exact("STRASSE").ignoring_case()),
            vec![18],
        ),
        (
            "folding reaches beyond ASCII: É is é",
            Locator::default().name_matching(TextMatch::exact("école").ignoring_case()),
            vec![19],
        ),
        (
            "É is not é without folding",
            Locator::default().name_matching(TextMatch::exact("école")),
            vec![],
        ),
        (
            "value, ignoring case",
            Locator::default().value(TextMatch::contains("hello w").ignoring_case()),
            vec![17],
        ),
        (
            "description is the hint",
            Locator::default().description(TextMatch::prefix("Saves a copy")),
            vec![3],
        ),
        ("identifier", Locator::identifier("delete"), vec![9, 12]),
        ("identifier is exact", Locator::identifier("del"), vec![]),
        (
            "checked",
            Locator::default().state(state(|s| s.checked = Some(Checked::True))),
            vec![13],
        ),
        (
            "unchecked",
            Locator::default().state(state(|s| s.checked = Some(Checked::False))),
            vec![14],
        ),
        (
            "expanded",
            Locator::default().state(state(|s| s.expanded = Some(true))),
            vec![16],
        ),
        (
            "collapsed",
            Locator::default().state(state(|s| s.expanded = Some(false))),
            vec![15],
        ),
        (
            "selected",
            Locator::default().state(state(|s| s.selected = Some(true))),
            vec![7],
        ),
        (
            "not selected needs a selectable node",
            Locator::default().state(state(|s| s.selected = Some(false))),
            vec![6, 8, 11],
        ),
        (
            "disabled, itself or through an ancestor",
            Locator::default().state(state(|s| s.disabled = Some(true))),
            vec![8, 23, 24],
        ),
        (
            "focused",
            Locator::default().state(state(|s| s.focused = Some(true))),
            vec![17],
        ),
        (
            "every criterion given must hold",
            Locator::role(Role::Button)
                .name_matching(TextMatch::contains("Save"))
                .state(state(|s| s.disabled = Some(false))),
            vec![2, 3],
        ),
        (
            "within a list",
            Locator::identifier("delete").within(inbox()),
            vec![9],
        ),
        (
            "within a list that is not there",
            Locator::identifier("delete").within(Locator::named("Drafts")),
            vec![],
        ),
        (
            "within reaches every descendant",
            Locator::role(Role::ListItem).within(inbox()),
            vec![6, 7, 8],
        ),
        (
            "has a descendant",
            Locator::role(Role::List).has(Locator::identifier("delete")),
            vec![5, 10],
        ),
        (
            "has a particular descendant",
            Locator::role(Role::List).has(Locator::named("Gamma")),
            vec![5],
        ),
        (
            "has a descendant nothing offers",
            Locator::role(Role::List).has(Locator::role(Role::Link)),
            vec![],
        ),
        (
            "within and has chain",
            Locator::role(Role::Button)
                .within(Locator::role(Role::List).has(Locator::named("Beta"))),
            vec![9],
        ),
        (
            "a node is not its own scope",
            Locator::named("Inbox").within(inbox()),
            vec![],
        ),
        (
            "a lifted container is no scope, its child is under the window",
            Locator::named("Inner").within(Locator::named("Mail")),
            vec![21],
        ),
        (
            "a hidden node is never found",
            Locator::named("Ghost"),
            vec![],
        ),
        (
            "this window",
            Locator::named("Save").in_window(window()),
            vec![2, 4],
        ),
    ];
    for (row, locator, expected) in table {
        assert_eq!(located(&f, locator), expected, "{row}");
    }
}

#[test]
fn a_locator_for_another_window_matches_nothing() {
    let f = mail();
    let other = flui_protocol::WindowId::from_u64(2).expect("non-zero");
    assert_eq!(
        located(&f, Locator::named("Save").in_window(other)),
        Vec::<u32>::new()
    );
}

/// `pick` narrows a listing to the match it names; strict lists them all.
#[test]
fn pick_names_one_match_in_tree_order() {
    let f = mail();
    let saves = || Locator::default().name_matching(TextMatch::contains("Save"));
    let table: Vec<(&str, Pick, Vec<u32>)> = vec![
        ("strict lists every match", Pick::Strict, vec![2, 3, 4]),
        ("first", Pick::First, vec![2]),
        ("last", Pick::Last, vec![4]),
        ("nth from 0", Pick::Nth(1), vec![3]),
        ("nth past the last", Pick::Nth(3), vec![]),
    ];
    for (row, pick, expected) in table {
        assert_eq!(located(&f, saves().pick(pick)), expected, "{row}");
    }
}

#[test]
fn max_matches_keeps_the_first_and_counts_the_rest() {
    let f = mail();
    let mut params =
        LocateParams::new(Locator::default().name_matching(TextMatch::contains("Save")));
    params.max_matches = Some(1);
    let matches = f.owner.locate_wire(&params, placement()).expect("reads");
    assert_eq!(matches.matches.len(), 1);
    assert_eq!(matches.matches[0].element, element(2));
    assert_eq!(matches.truncated, 2);
}

#[test]
fn a_match_carries_the_node_and_the_revision_it_was_read_on() {
    let mut f = mail();
    let found = f
        .owner
        .resolve_wire_locator(&Locator::identifier("save"), placement())
        .expect("one match");
    assert_eq!(found.element, element(2));
    assert_eq!(found.node.name.as_deref(), Some("Save"));
    assert_eq!(found.node.role, Role::Button);
    assert_eq!(found.revision, f.owner.revision());
    let first = f.owner.revision();

    f.owner.flush();
    assert_eq!(f.owner.revision(), first, "a flush with nothing to publish");

    f.owner
        .get_mut(f.id_of(2))
        .expect("in the tree")
        .config_mut()
        .set_label("Store");
    f.owner.flush();
    assert!(
        f.owner.revision() > first,
        "a publication that changed the tree"
    );
    let found = f
        .owner
        .resolve_wire_locator(&Locator::identifier("save"), placement())
        .expect("one match");
    assert_eq!(found.node.name.as_deref(), Some("Store"));
    assert_eq!(found.revision, f.owner.revision());
}

#[test]
fn resolving_a_strict_locator_with_several_matches_is_ambiguous() {
    let f = mail();
    let error = f
        .owner
        .resolve_wire_locator(&Locator::identifier("delete"), placement())
        .expect_err("two buttons carry the identifier");
    let WireLocateError::Ambiguous { count, candidates } = error else {
        panic!("expected ambiguous, got {error:?}");
    };
    assert_eq!(count, 2);
    assert_eq!(candidates.len(), 2);
    assert!(
        candidates
            .iter()
            .all(|line| line.starts_with("- button \"Delete\"")),
        "{candidates:?}"
    );
    assert!(
        candidates[0].contains(&element(9).element.to_string()),
        "{candidates:?}"
    );
}

#[test]
fn ambiguity_lists_at_most_ten_candidates() {
    let mut f = Fixture::new();
    f.add(None, 1, (0.0, 0.0, 400.0, 300.0), |c| c.set_label("Rows"));
    for index in 2..=14 {
        f.add(Some(1), index, (0.0, f64::from(index), 10.0, 1.0), |c| {
            c.set_role(SemanticsRole::Button);
            c.set_label("Row");
        });
    }
    f.flush();
    let error = f
        .owner
        .resolve_wire_locator(&Locator::named("Row"), placement())
        .expect_err("thirteen rows");
    let WireLocateError::Ambiguous { count, candidates } = error else {
        panic!("expected ambiguous, got {error:?}");
    };
    assert_eq!((count, candidates.len()), (13, 10));
}

#[test]
fn a_choice_made_with_pick_resolves() {
    let f = mail();
    let resolve = |pick| {
        f.owner
            .resolve_wire_locator(&Locator::identifier("delete").pick(pick), placement())
            .map(|found| found.element)
    };
    assert_eq!(resolve(Pick::First), Ok(element(9)));
    assert_eq!(resolve(Pick::Last), Ok(element(12)));
    assert_eq!(resolve(Pick::Nth(1)), Ok(element(12)));
    assert!(matches!(
        resolve(Pick::Nth(2)),
        Err(WireLocateError::NotFound { .. })
    ));
}

/// A miss names what was nearly meant, never one of them for the caller: the
/// closest candidates by role, name distance and identifier.
#[test]
fn a_miss_lists_the_nearest_elements() {
    let f = mail();
    let not_found = |locator: Locator| match f.owner.resolve_wire_locator(&locator, placement()) {
        Err(WireLocateError::NotFound { nearest }) => nearest,
        other => panic!("expected not found, got {other:?}"),
    };

    // A typo in the name of a button: that button first, then the other
    // buttons it shares the role with.
    let nearest = not_found(Locator::role(Role::Button).name("Savee"));
    assert!(nearest[0].starts_with("- button \"Save\""), "{nearest:?}");
    assert!(nearest.len() <= 10);

    // The right element in the wrong scope: both holders of every criterion
    // come before anything that only looks similar.
    let nearest = not_found(Locator::named("Save").within(Locator::named("Archive")));
    assert!(nearest[0].starts_with("- button \"Save\""), "{nearest:?}");
    assert!(nearest[1].starts_with("- link \"Save\""), "{nearest:?}");

    // An identifier that is not there lists nothing: no criterion holds for
    // anyone, and no name was given to compare.
    assert_eq!(
        not_found(Locator::identifier("remove")),
        Vec::<String>::new()
    );
}

#[test]
fn a_handle_is_not_rebound_when_a_slot_is_reused() {
    let mut f = mail();
    let old: ElementRef = f
        .owner
        .resolve_wire_locator(&Locator::identifier("save"), placement())
        .expect("one match")
        .element;
    assert_eq!(old, element(2));

    // The button is replaced by one in the same slot, a generation later.
    let replaced = f.id_of(2);
    f.owner.remove(replaced);
    f.add_generation(Some(1), 2, 6, (20.0, 0.0, 9.0, 9.0), |c| {
        c.set_role(SemanticsRole::Button);
        c.set_label("Save");
        c.set_identifier("save");
    });
    f.flush();
    let new = f
        .owner
        .resolve_wire_locator(&Locator::identifier("save"), placement())
        .expect("one match")
        .element;
    assert_ne!(new, old, "the replacement is a different element");
    assert_eq!(
        new,
        crate::fixture::element_of(crate::fixture::render_id(2, 6))
    );
}
