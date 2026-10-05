use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use flui_foundation::geometry::Rect as LogicalRect;
use flui_foundation::{RenderId, SemanticsId};
use flui_protocol::outline;

use super::*;
use crate::accesskit_translation::resolve_role;
use crate::configuration::SemanticsConfiguration;
use crate::flags::SemanticsFlag;
use crate::node::SemanticsNode;
use crate::role::SemanticsRole;
use crate::update::SemanticsNodeData;

fn render_id(index: u32) -> RenderId {
    RenderId::new_gen(
        index,
        NonZeroU32::new(3).expect("fixture generation is non-zero"),
    )
}

/// The wire handle of the fixture node built from `render_id(index)`.
fn e(index: u32) -> ElementId {
    ElementId::from_u64(AccessibilityNodeId::from(render_id(index)).as_u64())
        .expect("an accessibility id is non-zero")
}

fn noop() -> crate::SemanticsActionHandler {
    Arc::new(|_, _| {})
}

fn placement() -> Placement {
    Placement::new(
        DevicePixelRatio::new(2.0).expect("2 is a ratio"),
        WindowId::from_u64(1).expect("non-zero"),
    )
}

/// A semantics owner assembled by hand, one node per call.
struct Fixture {
    owner: SemanticsOwner,
}

impl Fixture {
    fn new() -> Self {
        Self {
            owner: SemanticsOwner::new_without_callback(),
        }
    }

    fn add(
        &mut self,
        parent: Option<SemanticsId>,
        index: u32,
        configure: impl FnOnce(&mut SemanticsConfiguration),
    ) -> SemanticsId {
        let mut node = SemanticsNode::new().with_source_render_id(render_id(index));
        node.set_rect(LogicalRect::from_xywh(0.0, 0.0, 100.0, 50.0));
        configure(node.config_mut());
        let id = self.owner.insert(node);
        match parent {
            Some(parent) => self.owner.add_child(parent, id),
            None => self.owner.set_root(Some(id)),
        }
        id
    }

    fn read(&self, query: &ReadQuery) -> Tree {
        self.owner
            .read_wire(query, placement())
            .expect("the fixture tree reads")
    }

    fn only(&self, element: ElementId) -> Node {
        let tree = self.read(&ReadQuery::new().with_root(element));
        tree.roots.into_iter().next().expect("one root")
    }
}

fn published_role(role: SemanticsRole, flags: &[SemanticsFlag], label: bool) -> accesskit::Role {
    resolve_role(&SemanticsNodeData {
        role,
        flags: flags.iter().fold(0, |bits, flag| bits | (*flag as u64)),
        label: label.then(|| "label".into()),
        ..Default::default()
    })
}

/// ADR-0095's "Mapping pinned": every semantics role reads as a wire role
/// other than `unknown`, except the documented ones. `none` declares no role,
/// and `drag_handle` and `hot_key` have no AccessKit counterpart; all three
/// publish a `GenericContainer`, which the wire lifts into its parent rather
/// than naming.
#[test]
fn every_role_but_the_documented_ones_reads_as_a_wire_role() {
    let documented = [
        SemanticsRole::None,
        SemanticsRole::DragHandle,
        SemanticsRole::HotKey,
    ];
    for &role in SemanticsRole::ALL {
        let published = published_role(role, &[], false);
        if documented.contains(&role) {
            assert_eq!(
                published,
                accesskit::Role::GenericContainer,
                "{role} has no wire node of its own"
            );
            continue;
        }
        assert_ne!(published, accesskit::Role::GenericContainer, "{role}");
        assert_ne!(
            wire_role(published),
            Role::Unknown,
            "{role} publishes {published:?}, which reads as unknown"
        );
    }
}

/// The controls a flag identifies read as the role UI Automation reports for
/// them: a multiline field is an `Edit` like any other, a keyboard key a
/// `Pane`, a header a `Group`.
#[test]
fn every_role_bearing_flag_reads_as_the_role_uia_reports() {
    use SemanticsFlag as F;
    let table: &[(&[SemanticsFlag], bool, Role)] = &[
        (&[F::IsButton], false, Role::Button),
        (&[F::IsLink], false, Role::Link),
        (&[F::IsTextField], false, Role::TextInput),
        (&[F::IsTextField, F::IsMultiline], false, Role::TextInput),
        (&[F::IsTextField, F::IsObscured], false, Role::PasswordInput),
        (&[F::IsSlider], false, Role::Slider),
        (&[F::IsKeyboardKey], false, Role::Pane),
        (&[F::IsImage], false, Role::Image),
        (&[F::IsHeader], false, Role::Group),
        (&[F::HasCheckedState], false, Role::CheckBox),
        (
            &[F::HasCheckedState, F::IsInMutuallyExclusiveGroup],
            false,
            Role::RadioButton,
        ),
        (&[F::HasToggledState], false, Role::Switch),
        (&[], true, Role::Label),
    ];
    for &(flags, label, expected) in table {
        let published = published_role(SemanticsRole::None, flags, label);
        assert_eq!(
            wire_role(published),
            expected,
            "{flags:?} (label: {label}) publishes {published:?}"
        );
    }
}

/// Every AccessKit role FLUI publishes, with the UI Automation control type
/// `accesskit_windows` 0.35 gives it (`node.rs` `control_type`) and the role
/// the desktop server reads from that control type (`uia.rs` `role_of`,
/// `refine_role`; `role.rs` `role_from_aria`). Transcribed from those
/// sources, not derived from `wire_role`.
/// The table below and `wire_role` were transcribed from one release of
/// `accesskit_windows`, which this crate cannot depend on. Bumping the adapter
/// fails here until the transcription is checked against the new release and
/// [`WIRE_ROLE_TRANSCRIBED_FROM`] moves with it.
#[test]
fn the_role_fold_was_transcribed_from_the_locked_windows_adapter() {
    let lock_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock");
    let lock = std::fs::read_to_string(lock_path)
        .unwrap_or_else(|error| panic!("reading {lock_path}: {error}"));
    let locked: Vec<&str> = lock
        .split("[[package]]")
        .filter(|entry| {
            entry
                .lines()
                .any(|line| line.trim() == r#"name = "accesskit_windows""#)
        })
        .filter_map(|entry| {
            entry.lines().find_map(|line| {
                line.trim()
                    .strip_prefix("version = \"")
                    .and_then(|rest| rest.strip_suffix('"'))
            })
        })
        .collect();
    assert_eq!(
        locked,
        [WIRE_ROLE_TRANSCRIBED_FROM],
        "the workspace locks a different accesskit_windows: re-check `wire_role`,          `offers_selection_item`, `reads_selected` and the table in          `wire_role_matches_the_windows_adapter_for_every_role_flui_publishes` against it,          then update WIRE_ROLE_TRANSCRIBED_FROM"
    );
}

#[test]
fn wire_role_matches_the_windows_adapter_for_every_role_flui_publishes() {
    use SemanticsFlag as F;
    use accesskit::Role as A;
    let table: &[(A, &str, Role)] = &[
        (A::AlertDialog, "Window, IsDialog", Role::Dialog),
        (A::Dialog, "Window, IsDialog", Role::Dialog),
        (A::Window, "Window", Role::Window),
        (A::Tab, "TabItem", Role::Tab),
        (A::TabList, "Tab", Role::TabList),
        (A::TabPanel, "Pane", Role::Pane),
        (A::Keyboard, "Pane", Role::Pane),
        (A::Table, "Table", Role::Table),
        (A::Cell, "DataItem, aria cell", Role::Cell),
        (A::Row, "DataItem, aria row", Role::Row),
        (
            A::ColumnHeader,
            "DataItem, aria columnheader",
            Role::ColumnHeader,
        ),
        (A::RadioGroup, "Group", Role::Group),
        (A::Complementary, "Group", Role::Group),
        (A::ContentInfo, "Group", Role::Group),
        (A::Main, "Group", Role::Group),
        (A::Navigation, "Group", Role::Group),
        (A::Region, "Group", Role::Group),
        (A::Form, "Group", Role::Group),
        (A::Header, "Group", Role::Group),
        (A::GenericContainer, "Group", Role::Group),
        (A::Menu, "Menu", Role::Menu),
        (A::MenuBar, "MenuBar", Role::MenuBar),
        (A::MenuItem, "MenuItem", Role::MenuItem),
        (A::MenuItemCheckBox, "CheckBox", Role::CheckBox),
        (A::MenuItemRadio, "RadioButton", Role::RadioButton),
        (A::Alert, "Text", Role::Label),
        (A::Label, "Text", Role::Label),
        (A::Status, "StatusBar", Role::Status),
        (A::List, "List", Role::List),
        (A::ListItem, "ListItem", Role::ListItem),
        (A::SpinButton, "Spinner", Role::SpinButton),
        (A::ComboBox, "ComboBox", Role::ComboBox),
        (A::Tooltip, "ToolTip", Role::Tooltip),
        (A::ProgressIndicator, "ProgressBar", Role::ProgressIndicator),
        (A::Switch, "Button, aria switch", Role::Switch),
        (A::CheckBox, "CheckBox", Role::CheckBox),
        (A::RadioButton, "RadioButton", Role::RadioButton),
        (A::Button, "Button", Role::Button),
        (A::Link, "Hyperlink", Role::Link),
        (A::TextInput, "Edit", Role::TextInput),
        (A::MultilineTextInput, "Edit", Role::TextInput),
        (A::PasswordInput, "Edit, IsPassword", Role::PasswordInput),
        (A::Slider, "Slider", Role::Slider),
        (A::Image, "Image", Role::Image),
    ];

    // What FLUI publishes: every explicit role, every role-bearing flag,
    // static text, a bare container, and the root's window.
    let mut published: Vec<A> = SemanticsRole::ALL
        .iter()
        .map(|&role| published_role(role, &[], false))
        .collect();
    for flags in [
        &[F::IsButton][..],
        &[F::IsLink],
        &[F::IsTextField],
        &[F::IsTextField, F::IsMultiline],
        &[F::IsTextField, F::IsObscured],
        &[F::IsSlider],
        &[F::IsKeyboardKey],
        &[F::IsImage],
        &[F::IsHeader],
        &[F::HasCheckedState],
        &[F::HasCheckedState, F::IsInMutuallyExclusiveGroup],
        &[F::HasToggledState],
    ] {
        published.push(published_role(SemanticsRole::None, flags, false));
    }
    published.push(published_role(SemanticsRole::None, &[], true));
    published.push(A::Window);

    for role in &published {
        assert!(
            table.iter().any(|(listed, _, _)| listed == role),
            "FLUI publishes {role:?}, which the table does not pin"
        );
    }
    for &(role, control_type, expected) in table {
        assert_eq!(
            wire_role(role),
            expected,
            "{role:?} is UI Automation's {control_type}"
        );
    }
}

/// Every tool of the ADR-0080 wire vocabulary, as accesskit_windows 0.35.1
/// turns its UI Automation call into an AccessKit action (`node.rs`), and the
/// FLUI action that action reaches. The in-process route
/// (`semantics_action_for_wire`) must reach the same FLUI action, so an
/// agent's `invoke` means one thing whichever backend carries it.
///
/// Text `set_value` lands on `SetText`; numeric requests use their typed
/// route, and expand/collapse retain their explicit direction.
#[test]
fn every_wire_action_routes_to_a_semantics_action() {
    use crate::accesskit_translation::semantics_action_for;
    use accesskit::Action as Ak;

    let table: &[(ActionName, Ak, SemanticsAction)] = &[
        // `Invoke` -> `click()` -> Click (node.rs:1340-1343, 953).
        (ActionName::Invoke, Ak::Click, SemanticsAction::Tap),
        // `Toggle` -> `click()` -> Click (node.rs:1336-1338, 953).
        (ActionName::Toggle, Ak::Click, SemanticsAction::Tap),
        // `Value`/`RangeValue.SetValue` -> SetValue (node.rs:1352, 1366).
        (ActionName::SetValue, Ak::SetValue, SemanticsAction::SetText),
        // `SelectionItem.Select` -> Click (node.rs:977-997).
        (ActionName::Select, Ak::Click, SemanticsAction::Tap),
        // `SetFocus` -> Focus (node.rs:1130).
        (ActionName::Focus, Ak::Focus, SemanticsAction::Focus),
        // `ExpandCollapse` -> Expand / Collapse, only toward the state the
        // node lacks (node.rs:955-975).
        (ActionName::Expand, Ak::Expand, SemanticsAction::Expand),
        (
            ActionName::Collapse,
            Ak::Collapse,
            SemanticsAction::Collapse,
        ),
        // `ScrollItem` -> ScrollIntoView (node.rs:1374).
        (
            ActionName::ScrollIntoView,
            Ak::ScrollIntoView,
            SemanticsAction::ShowOnScreen,
        ),
    ];

    let listed: Vec<ActionName> = table.iter().map(|(name, _, _)| *name).collect();
    assert_eq!(
        listed,
        ActionName::ALL,
        "one row per wire action, in ActionName::ALL's order: a wire action \
         without a row here has no pinned route into FLUI",
    );
    for &(name, platform, expected) in table {
        assert_eq!(
            semantics_action_for(platform),
            Some(expected),
            "`{name}` arrives as {platform:?} and must reach {expected:?}",
        );
        assert_eq!(
            semantics_action_for_wire(name),
            Some(expected),
            "`{name}` routes differently in process than through UI Automation",
        );
    }
}

#[test]
fn a_read_before_the_first_assembly_is_no_tree_and_busy() {
    let owner = SemanticsOwner::new_without_callback();
    let read = owner.read_wire(&ReadQuery::new(), placement());
    assert_eq!(read, Err(WireReadError::NoTree));
}

/// A `GenericContainer` is lifted into its parent and a hidden node takes its
/// subtree with it, as `accesskit_consumer::common_filter` does for every
/// adapter.
#[test]
fn generic_containers_are_lifted_and_hidden_subtrees_dropped() {
    let mut f = Fixture::new();
    let root = f.add(None, 1, |_| {});
    let container = f.add(Some(root), 2, |_| {});
    f.add(Some(container), 3, |c| {
        c.set_button(true);
        c.set_label("Kept");
    });
    let hidden = f.add(Some(root), 4, |c| {
        c.set_hidden(true);
        c.set_label("Hidden group");
    });
    f.add(Some(hidden), 5, |c| {
        c.set_button(true);
        c.set_label("Hidden button");
    });

    let tree = f.read(&ReadQuery::new());
    assert_eq!(
        outline(&tree.roots),
        format!(
            "- window [ref={}] [window=w1]\n  - button \"Kept\" [ref={}]\n",
            e(1),
            e(3)
        )
    );
    assert_eq!((tree.count, tree.truncated), (2, false));
    assert_eq!(tree.roots[0].native_role, "Window");
    assert_eq!(
        f.owner
            .read_wire(&ReadQuery::new().with_root(e(2)), placement()),
        Err(WireReadError::NotFound { element: e(2) }),
        "a lifted container has no handle of its own"
    );
}

#[test]
fn focus_is_reported_where_a_node_claims_it_and_nowhere_else() {
    let mut f = Fixture::new();
    let root = f.add(None, 1, |_| {});
    f.add(Some(root), 2, |c| {
        c.set_button(true);
        c.set_label("Idle");
    });
    assert!(
        !f.read(&ReadQuery::new()).roots[0].focused,
        "AccessKit's fallback focus on the root is not a claim"
    );
    f.add(Some(root), 3, |c| {
        c.set_text_field(true);
        c.set_focused(true);
    });
    let tree = f.read(&ReadQuery::new());
    let focused: Vec<ElementId> = tree.roots[0]
        .children
        .iter()
        .filter(|n| n.focused)
        .map(|n| n.id)
        .collect();
    assert_eq!(focused, [e(3)]);
    assert!(!tree.roots[0].focused);
}

#[test]
fn advertised_actions_follow_the_uia_patterns() {
    use ActionName as N;
    let mut f = Fixture::new();
    let root = f.add(None, 1, |_| {});
    f.add(Some(root), 2, |c| {
        c.set_button(true);
        c.set_label("Go");
        c.add_action(SemanticsAction::Tap, noop());
        c.add_action(SemanticsAction::Focus, noop());
    });
    f.add(Some(root), 3, |c| {
        c.set_checked(Some(false));
        c.add_action(SemanticsAction::Tap, noop());
    });
    f.add(Some(root), 4, |c| {
        c.set_checked(Some(true));
        c.set_in_mutually_exclusive_group(true);
        c.add_action(SemanticsAction::Tap, noop());
    });
    f.add(Some(root), 5, |c| {
        c.set_text_field(true);
        c.set_value("draft");
        c.add_action(SemanticsAction::SetText, noop());
    });
    f.add(Some(root), 6, |c| {
        c.set_text_field(true);
        c.set_read_only(true);
        c.set_value("fixed");
        c.add_action(SemanticsAction::SetText, noop());
    });
    f.add(Some(root), 7, |c| {
        c.set_button(true);
        c.set_expanded(false);
        c.add_action(SemanticsAction::Expand, noop());
        c.add_action(SemanticsAction::Collapse, noop());
        c.add_action(SemanticsAction::Tap, noop());
    });
    f.add(Some(root), 8, |c| {
        c.set_label("Row");
        c.add_action(SemanticsAction::ShowOnScreen, noop());
    });
    f.add(Some(root), 9, |c| {
        c.set_text_field(true);
        c.set_obscured(true);
        c.set_value("hunter2");
        c.add_action(SemanticsAction::SetText, noop());
    });

    let button = f.only(e(2));
    assert_eq!(button.actions, [N::Invoke, N::Focus]);
    assert!(button.focusable);

    let check = f.only(e(3));
    assert_eq!(check.actions, [N::Toggle], "a toggle is not invocable");
    assert_eq!(check.checked, Some(Checked::False));

    let radio = f.only(e(4));
    assert_eq!(
        radio.actions,
        [N::Select],
        "a radio selects, it does not toggle"
    );
    assert_eq!((radio.checked, radio.selected), (None, Some(true)));

    let field = f.only(e(5));
    assert_eq!(field.actions, [N::SetValue]);
    assert_eq!(field.value.as_deref(), Some("draft"));

    assert_eq!(f.only(e(6)).actions, [], "a read-only field takes no value");

    let expandable = f.only(e(7));
    assert_eq!(expandable.actions, [N::Expand]);
    assert_eq!(expandable.expanded, Some(false));

    let text = f.only(e(8));
    assert_eq!(text.role, Role::Label);
    assert_eq!(
        text.name.as_deref(),
        Some("Row"),
        "static text is named by its text"
    );
    assert_eq!(text.value, None, "and has no value of its own");
    assert_eq!(text.actions, [N::ScrollIntoView]);

    let password = f.only(e(9));
    assert_eq!(password.role, Role::PasswordInput);
    assert_eq!(password.value, None, "a password's value is never read");
    assert_eq!(password.actions, [N::SetValue]);
}

/// An in-process expand reads the published expanded flag and refuses the
/// already-published state. Pending requests retain explicit direction.
#[test]
fn expand_on_an_expanded_node_is_action_unsupported() {
    let mut f = Fixture::new();
    let root = f.add(None, 1, |_| {});
    f.add(Some(root), 2, |c| {
        c.set_button(true);
        c.set_expanded(true);
        c.add_action(SemanticsAction::Expand, noop());
        c.add_action(SemanticsAction::Collapse, noop());
    });

    let expand = f
        .owner
        .resolve_wire_action(&ActionRequest::new(e(2), ActionName::Expand));
    assert_eq!(
        expand,
        Err(WireActionError::ActionUnsupported {
            element: e(2),
            action: ActionName::Expand
        })
    );

    let collapse = f
        .owner
        .resolve_wire_action(&ActionRequest::new(e(2), ActionName::Collapse))
        .expect("collapse is the transition an expanded node allows");
    assert_eq!(collapse.action, SemanticsAction::Collapse);
    assert_eq!(collapse.arguments, None);
}

#[test]
fn set_value_reaches_set_text_with_its_text() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&received);
    let mut f = Fixture::new();
    let root = f.add(None, 1, |_| {});
    f.add(Some(root), 2, move |c| {
        c.set_text_field(true);
        c.add_action(
            SemanticsAction::SetText,
            Arc::new(move |action, arguments| {
                sink.lock()
                    .expect("the test lock is not poisoned")
                    .push((action, arguments));
            }),
        );
    });

    let request = f
        .owner
        .resolve_wire_action(&ActionRequest::set_value(e(2), "hello"))
        .expect("the field advertises set_value");
    assert_eq!(request.action, SemanticsAction::SetText);
    f.owner
        .resolve_action(request)
        .expect("the resolved request reaches the node's handler")
        .invoke();
    assert_eq!(
        *received.lock().expect("the test lock is not poisoned"),
        [(
            SemanticsAction::SetText,
            Some(ActionArgs::SetText {
                text: "hello".into()
            })
        )]
    );

    let without_value = f
        .owner
        .resolve_wire_action(&ActionRequest::new(e(2), ActionName::SetValue));
    assert!(
        matches!(without_value, Err(WireActionError::InvalidArgument { .. })),
        "{without_value:?}"
    );
    let mut numeric = Fixture::new();
    let root = numeric.add(None, 1, |_| {});
    numeric.add(Some(root), 2, |c| {
        c.set_numeric_range(crate::NumericRange::new(0.0, 0.0, 10.0, 1.0).expect("finite fixture"));
        c.add_action(SemanticsAction::SetNumericValue, noop());
    });
    let request = numeric
        .owner
        .resolve_wire_action(&ActionRequest::set_value(e(2), "2.375"))
        .expect("numeric range advertises set_value");
    assert_eq!(request.action, SemanticsAction::SetNumericValue);
    assert_eq!(
        request.arguments,
        Some(ActionArgs::SetNumericValue { value: 2.375 })
    );
    let _ = numeric
        .owner
        .resolve_action(request)
        .expect("exact fraction admitted despite step");
    for text in ["NaN", "inf", "not a number", "-1", "11"] {
        assert!(
            matches!(
                numeric
                    .owner
                    .resolve_wire_action(&ActionRequest::set_value(e(2), text)),
                Err(WireActionError::InvalidArgument { .. })
            ),
            "numeric wire input {text:?} was admitted"
        );
    }
}

#[test]
fn a_disabled_node_refuses_with_disabled() {
    let mut f = Fixture::new();
    let root = f.add(None, 1, |_| {});
    f.add(Some(root), 2, |c| {
        c.set_button(true);
        c.set_enabled(Some(false));
        c.add_action(SemanticsAction::Tap, noop());
    });
    assert!(f.only(e(2)).disabled);
    let refused = f
        .owner
        .resolve_wire_action(&ActionRequest::new(e(2), ActionName::Invoke));
    assert_eq!(refused, Err(WireActionError::Disabled { element: e(2) }));

    let gone = f
        .owner
        .resolve_wire_action(&ActionRequest::new(e(40), ActionName::Invoke));
    assert_eq!(gone, Err(WireActionError::NotFound { element: e(40) }));
}

#[test]
fn read_honours_max_depth_and_max_nodes_and_says_truncated() {
    let mut f = Fixture::new();
    let root = f.add(None, 1, |_| {});
    let first = f.add(Some(root), 2, |c| c.set_label("first"));
    f.add(Some(first), 5, |c| c.set_label("first.a"));
    f.add(Some(first), 6, |c| c.set_label("first.b"));
    f.add(Some(root), 3, |c| c.set_label("second"));
    f.add(Some(root), 4, |c| c.set_label("third"));

    let whole = f.read(&ReadQuery::new());
    assert_eq!((whole.count, whole.truncated), (6, false));

    let shallow = f.read(&ReadQuery::new().with_max_depth(0));
    assert_eq!((shallow.count, shallow.truncated), (1, true));
    assert_eq!(shallow.roots[0].omitted_children, Some(3));

    let one_level = f.read(&ReadQuery::new().with_max_depth(1));
    assert_eq!((one_level.count, one_level.truncated), (4, true));
    assert_eq!(one_level.roots[0].children[0].omitted_children, Some(2));

    let budget = f.read(&ReadQuery::new().with_max_nodes(3));
    assert_eq!((budget.count, budget.truncated), (3, true));
    assert_eq!(
        outline(&budget.roots),
        format!(
            "- window [ref={}] [window=w1]\n  - label \"first\" [ref={}]\n    - label \
             \"first.a\" [ref={}]\n    - … 1 more children not read\n  - … 2 more \
             children not read\n",
            e(1),
            e(2),
            e(5)
        )
    );

    let scoped = f.read(&ReadQuery::new().with_root(e(2)));
    assert_eq!(scoped.count, 3);
    assert_eq!(
        scoped.roots[0].window, None,
        "only a window's root names it"
    );
}

#[test]
fn a_read_tree_round_trips_through_json() {
    let mut f = Fixture::new();
    let root = f.add(None, 1, |_| {});
    let button = f.add(Some(root), 2, |c| {
        c.set_button(true);
        c.set_label("Save");
        c.add_action(SemanticsAction::Tap, noop());
    });
    f.owner
        .get_mut(button)
        .expect("the button was just added")
        .set_rect(LogicalRect::from_ltrb(10.0, 20.25, 40.5, 41.0));

    let tree = f.read(&ReadQuery::new());
    let json = serde_json::to_value(&tree).expect("a tree serializes");
    assert_eq!(json["protocol"], "0.1");
    assert_eq!(
        json["roots"][0]["children"][0],
        serde_json::json!({
            "id": e(2).to_string(),
            "role": "button",
            "native_role": "Button",
            "name": "Save",
            "surface_rect": {"x": 20, "y": 40, "width": 61, "height": 42},
            "actions": ["invoke"],
        }),
        "the surface rect covers the logical bounds at a device pixel ratio of 2, and no          screen `rect` is claimed"
    );
    let back: Tree = serde_json::from_value(json).expect("a tree deserializes");
    assert_eq!(back, tree);
}
