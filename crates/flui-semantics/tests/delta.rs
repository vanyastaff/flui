//! What an action changed in the published tree (ADR-0238 §9, R16), through
//! the public API: nodes that appeared, nodes that are gone and nodes whose
//! description changed, matched by element id, with bounds left out unless
//! asked and a cap on what is reported.

use flui_protocol::{ChangedField, ElementId, SnapshotDelta};
use flui_semantics::{SemanticsRole, Toggled, TreeSnapshot};

use crate::fixture::{Fixture, element, element_of, placement, render_id};

/// What differs between the fixture trees, one row of the table below.
#[derive(Clone, Copy)]
struct Edit {
    rename_save: bool,
    value: &'static str,
    notify: bool,
    keep_gone_button: bool,
    reuse_gone_slot: bool,
    add_extra: bool,
    save_as_link: bool,
    move_save: bool,
}

const SAME: Edit = Edit {
    rename_save: false,
    value: "a",
    notify: false,
    keep_gone_button: true,
    reuse_gone_slot: false,
    add_extra: false,
    save_as_link: false,
    move_save: false,
};

/// A window with a button, a count, a check box, a text field and one more
/// button, as `edit` describes it. Index → what it is:
///
/// ```text
/// 1 window   2 button "Save"   3 text "Count: 0"   4 check box "Notify"
/// 5 text input "Name" = value   6 button "Later"   7 button "Extra" (added)
/// ```
fn tree(edit: Edit) -> TreeSnapshot {
    let mut f = Fixture::new();
    f.add(None, 1, (0.0, 0.0, 400.0, 300.0), |c| c.set_label("App"));
    let save = if edit.move_save {
        (100.0, 0.0, 50.0, 20.0)
    } else {
        (0.0, 0.0, 50.0, 20.0)
    };
    f.add(Some(1), 2, save, |c| {
        c.set_role(if edit.save_as_link {
            SemanticsRole::Link
        } else {
            SemanticsRole::Button
        });
        c.set_label(if edit.rename_save { "Saved" } else { "Save" });
    });
    f.add(Some(1), 3, (0.0, 30.0, 50.0, 20.0), |c| {
        c.set_label("Count: 0");
    });
    f.add(Some(1), 4, (0.0, 60.0, 50.0, 20.0), |c| {
        c.set_role(SemanticsRole::CheckBox);
        c.set_label("Notify");
        c.set_toggled(Some(if edit.notify {
            Toggled::On
        } else {
            Toggled::Off
        }));
    });
    f.add(Some(1), 5, (0.0, 90.0, 50.0, 20.0), |c| {
        c.set_role(SemanticsRole::TextInput);
        c.set_label("Name");
        c.set_value(edit.value);
    });
    if edit.keep_gone_button {
        let generation = if edit.reuse_gone_slot { 6 } else { 5 };
        f.add_generation(Some(1), 6, generation, (0.0, 120.0, 50.0, 20.0), |c| {
            c.set_role(SemanticsRole::Button);
            c.set_label("Later");
        });
    }
    if edit.add_extra {
        f.add(Some(1), 7, (0.0, 150.0, 50.0, 20.0), |c| {
            c.set_role(SemanticsRole::Button);
            c.set_label("Extra");
        });
    }
    f.flush();
    f.owner
        .snapshot_wire(placement())
        .expect("a flushed tree reads")
}

/// The fixture index of `id`, with `'` for the reused slot's later generation.
fn name_of(id: ElementId) -> String {
    (1..=7)
        .find_map(|index| (element(index).element == id).then(|| index.to_string()))
        .or_else(|| {
            (1..=7).find_map(|index| {
                (element_of(render_id(index, 6)).element == id).then(|| format!("{index}'"))
            })
        })
        .unwrap_or_else(|| panic!("{id} is no fixture node"))
}

/// `appeared [..] gone [..] changed [index:field,..]` for a delta.
fn summary(delta: &SnapshotDelta) -> String {
    let appeared: Vec<_> = delta.appeared.iter().map(|n| name_of(n.id)).collect();
    let gone: Vec<_> = delta.gone.iter().map(|g| name_of(g.element)).collect();
    let changed: Vec<_> = delta
        .changed
        .iter()
        .map(|change| {
            let fields: Vec<_> = change.changed.iter().map(|f| format!("{f:?}")).collect();
            format!("{}:{}", name_of(change.node.id), fields.join("+"))
        })
        .collect();
    format!(
        "appeared [{}] gone [{}] changed [{}]{}",
        appeared.join(","),
        gone.join(","),
        changed.join(","),
        if delta.truncated { " truncated" } else { "" }
    )
}

#[test]
fn a_delta_names_what_appeared_went_and_changed() {
    let before = tree(SAME);
    let rows: &[(&str, Edit, bool, &str)] = &[
        (
            "nothing changed",
            SAME,
            false,
            "appeared [] gone [] changed []",
        ),
        (
            "a name",
            Edit {
                rename_save: true,
                ..SAME
            },
            false,
            "appeared [] gone [] changed [2:Name]",
        ),
        (
            "a value",
            Edit {
                value: "ab",
                ..SAME
            },
            false,
            "appeared [] gone [] changed [5:Value]",
        ),
        (
            "a state",
            Edit {
                notify: true,
                ..SAME
            },
            false,
            "appeared [] gone [] changed [4:State]",
        ),
        (
            "a node removed",
            Edit {
                keep_gone_button: false,
                ..SAME
            },
            false,
            "appeared [] gone [6] changed []",
        ),
        (
            "a node added",
            Edit {
                add_extra: true,
                ..SAME
            },
            false,
            "appeared [7] gone [] changed []",
        ),
        (
            "a slot reused is gone and new, not changed",
            Edit {
                reuse_gone_slot: true,
                ..SAME
            },
            false,
            "appeared [6'] gone [6] changed []",
        ),
        (
            "a role changed is a node that appeared",
            Edit {
                save_as_link: true,
                ..SAME
            },
            false,
            "appeared [2] gone [] changed []",
        ),
        (
            "bounds alone are left out",
            Edit {
                move_save: true,
                ..SAME
            },
            false,
            "appeared [] gone [] changed []",
        ),
        (
            "bounds when asked",
            Edit {
                move_save: true,
                ..SAME
            },
            true,
            "appeared [] gone [] changed [2:Bounds]",
        ),
        (
            "several at once",
            Edit {
                rename_save: true,
                notify: true,
                keep_gone_button: false,
                add_extra: true,
                ..SAME
            },
            false,
            "appeared [7] gone [6] changed [2:Name,4:State]",
        ),
    ];
    for (what, edit, bounds, expected) in rows {
        let after = tree(*edit);
        assert_eq!(
            summary(&before.delta_to(&after, *bounds, 100)),
            *expected,
            "{what}"
        );
    }
}

#[test]
fn a_delta_carries_the_node_as_it_is_now_and_the_revisions_it_spans() {
    let before = tree(SAME);
    let after = tree(Edit {
        rename_save: true,
        ..SAME
    });
    let delta = before.delta_to(&after, false, 100);
    assert_eq!(delta.from, before.revision());
    assert_eq!(delta.to, after.revision());
    let change = &delta.changed[0];
    assert_eq!(change.node.name.as_deref(), Some("Saved"));
    assert!(
        change.node.children.is_empty(),
        "a changed node has no children"
    );
    assert_eq!(change.changed, vec![ChangedField::Name]);
}

#[test]
fn a_delta_past_its_cap_says_so_and_keeps_the_first_entries() {
    let before = tree(SAME);
    let after = tree(Edit {
        rename_save: true,
        notify: true,
        add_extra: true,
        ..SAME
    });
    let delta = before.delta_to(&after, false, 2);
    assert!(delta.truncated);
    assert_eq!(
        delta.appeared.len() + delta.gone.len() + delta.changed.len(),
        2,
        "{}",
        summary(&delta)
    );
    assert!(!before.delta_to(&after, false, 3).truncated);
}
