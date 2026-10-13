use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use flui_foundation::geometry::Rect as LogicalRect;
use flui_foundation::{RenderId, SemanticsId};
use flui_protocol::outline;

use super::*;
use crate::accesskit_translation::resolve_role;
use crate::configuration::SemanticsConfiguration;
use crate::node::SemanticsNode;
use crate::role::SemanticsRole;

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
    Arc::new(|_| {})
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

fn published_role(role: SemanticsRole, label: bool) -> accesskit::Role {
    let attrs = crate::attributes::SemanticsAttributes {
        role,
        label: label.then(|| "label".into()),
        ..Default::default()
    };
    resolve_role(&crate::accesskit_translation::PublishView {
        id: flui_foundation::RenderId::new(1).into(),
        attrs: &attrs,
        actions: 0,
        rect: flui_foundation::geometry::Rect::ZERO,
        children: smallvec::SmallVec::new(),
    })
}

/// ADR-0095's "Mapping pinned": every semantics role reads as a wire role
/// other than `unknown`, except `none`, which declares no role and publishes
/// a `GenericContainer` the wire lifts into its parent rather than naming.
#[test]
fn every_role_but_none_reads_as_a_wire_role() {
    for &role in SemanticsRole::ALL {
        let published = published_role(role, false);
        if role == SemanticsRole::None {
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

/// The interactive roles read as the role UI Automation reports for them: a
/// multi-line field is an `Edit` like any other, a heading `Text` restored by
/// its ARIA role, and a named node without a role is static text.
#[test]
fn every_interactive_role_reads_as_the_role_uia_reports() {
    use SemanticsRole as S;
    let table: &[(SemanticsRole, bool, Role)] = &[
        (S::Button, false, Role::Button),
        (S::Link, false, Role::Link),
        (S::TextInput, false, Role::TextInput),
        (S::MultilineTextInput, false, Role::TextInput),
        (S::PasswordInput, false, Role::PasswordInput),
        (S::Slider, false, Role::Slider),
        (S::Image, false, Role::Image),
        (S::Heading, false, Role::Heading),
        (S::CheckBox, false, Role::CheckBox),
        (S::RadioButton, false, Role::RadioButton),
        (S::Switch, false, Role::Switch),
        (S::None, true, Role::Label),
    ];
    for &(role, label, expected) in table {
        let published = published_role(role, label);
        assert_eq!(
            wire_role(published),
            expected,
            "{role} (label: {label}) publishes {published:?}"
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
    assert_eq!(
        crate::accesskit_translation::coverage_matrix::locked_versions("accesskit_windows"),
        [WIRE_ROLE_TRANSCRIBED_FROM],
        "the workspace locks a different accesskit_windows: re-check `wire_role`, \
         `offers_selection_item`, `reads_selected` and the table in \
         `wire_role_matches_the_windows_adapter_for_every_role_flui_publishes` against it, \
         then update WIRE_ROLE_TRANSCRIBED_FROM"
    );
}

#[test]
fn wire_role_matches_the_windows_adapter_for_every_role_flui_publishes() {
    use accesskit::Role as A;
    let table: &[(A, &str, Role)] = &[
        (A::AlertDialog, "Window, IsDialog", Role::Dialog),
        (A::Dialog, "Window, IsDialog", Role::Dialog),
        (A::Window, "Window", Role::Window),
        (A::Tab, "TabItem", Role::Tab),
        (A::TabList, "Tab", Role::TabList),
        (A::TabPanel, "Pane", Role::Pane),
        (A::ScrollView, "Pane", Role::Pane),
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
        (A::Heading, "Text, aria heading", Role::Heading),
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
        (A::ScrollBar, "ScrollBar", Role::ScrollBar),
        (A::Splitter, "Separator", Role::Splitter),
        (A::SearchInput, "Edit", Role::TextInput),
        (A::EmailInput, "Edit", Role::TextInput),
        (A::NumberInput, "Edit", Role::TextInput),
        (A::PhoneNumberInput, "Edit", Role::TextInput),
        (A::UrlInput, "Edit", Role::TextInput),
        (A::DateInput, "Edit", Role::TextInput),
        (A::TimeInput, "Edit", Role::TextInput),
        (A::ListBox, "List", Role::List),
        (A::ListBoxOption, "ListItem", Role::ListItem),
        (A::Tree, "Tree", Role::Tree),
        (A::TreeItem, "TreeItem", Role::TreeItem),
        (A::TreeGrid, "DataGrid", Role::Grid),
        (A::Grid, "DataGrid", Role::Grid),
        (A::GridCell, "DataItem, aria gridcell", Role::GridCell),
        (A::Toolbar, "ToolBar", Role::Toolbar),
        (A::RowGroup, "Group", Role::Group),
        (A::RowHeader, "DataItem, aria rowheader", Role::RowHeader),
        (A::Paragraph, "Group", Role::Group),
        (A::Figure, "Group", Role::Group),
        (A::Caption, "Text", Role::Label),
        (A::Article, "Group", Role::Group),
        (A::Feed, "Group", Role::Group),
        (A::Group, "Group", Role::Group),
        (A::Note, "Group", Role::Group),
        (A::Blockquote, "Group", Role::Group),
        (A::Code, "Text", Role::Label),
        (A::Emphasis, "Text", Role::Label),
        (A::Strong, "Text", Role::Label),
        (A::ContentInsertion, "Group", Role::Group),
        (A::ContentDeletion, "Group", Role::Group),
        (A::Term, "ListItem", Role::ListItem),
        (A::Definition, "Group", Role::Group),
        (A::Time, "Text", Role::Label),
        (A::Math, "Group", Role::Group),
        (A::Meter, "ProgressBar", Role::ProgressIndicator),
        (A::Document, "Document", Role::Document),
        (A::Application, "Pane", Role::Pane),
        (A::Banner, "Group", Role::Group),
        (A::Search, "Group", Role::Group),
        (A::Log, "Group", Role::Group),
        (A::Marquee, "Text", Role::Label),
        (A::Timer, "Pane", Role::Pane),
        (A::Pane, "Pane", Role::Pane),
    ];

    // What FLUI publishes: every role, static text, a bare container, and
    // the root's window.
    let mut published: Vec<A> = SemanticsRole::ALL
        .iter()
        .map(|&role| published_role(role, false))
        .collect();
    published.push(published_role(SemanticsRole::None, true));
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
        (ActionName::Invoke, Ak::Click, SemanticsAction::Click),
        // `Toggle` -> `click()` -> Click (node.rs:1336-1338, 953).
        (ActionName::Toggle, Ak::Click, SemanticsAction::Click),
        // `Value`/`RangeValue.SetValue` -> SetValue (node.rs:1352, 1366).
        (
            ActionName::SetValue,
            Ak::SetValue,
            SemanticsAction::SetValue,
        ),
        // `SelectionItem.Select` -> Click (node.rs:977-997).
        (ActionName::Select, Ak::Click, SemanticsAction::Click),
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
            SemanticsAction::ScrollIntoView,
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
        c.set_role(SemanticsRole::Button);
        c.set_label("Kept");
    });
    let hidden = f.add(Some(root), 4, |c| {
        c.set_hidden(true);
        c.set_label("Hidden group");
    });
    f.add(Some(hidden), 5, |c| {
        c.set_role(SemanticsRole::Button);
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
        c.set_role(SemanticsRole::Button);
        c.set_label("Idle");
    });
    assert!(
        !f.read(&ReadQuery::new()).roots[0].focused,
        "AccessKit's fallback focus on the root is not a claim"
    );
    f.add(Some(root), 3, |c| {
        c.set_role(SemanticsRole::TextInput);
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
        c.set_role(SemanticsRole::Button);
        c.set_label("Go");
        c.add_action(SemanticsAction::Click, noop());
        c.add_action(SemanticsAction::Focus, noop());
    });
    f.add(Some(root), 3, |c| {
        c.set_role(SemanticsRole::CheckBox);
        c.set_toggled(Some(crate::Toggled::Off));
        c.add_action(SemanticsAction::Click, noop());
    });
    f.add(Some(root), 4, |c| {
        c.set_role(SemanticsRole::RadioButton);
        c.set_toggled(Some(crate::Toggled::On));
        c.add_action(SemanticsAction::Click, noop());
    });
    f.add(Some(root), 5, |c| {
        c.set_role(SemanticsRole::TextInput);
        c.set_value("draft");
        c.add_action(SemanticsAction::SetValue, noop());
    });
    f.add(Some(root), 6, |c| {
        c.set_role(SemanticsRole::TextInput);
        c.set_read_only(true);
        c.set_value("fixed");
        c.add_action(SemanticsAction::SetValue, noop());
    });
    f.add(Some(root), 7, |c| {
        c.set_role(SemanticsRole::Button);
        c.set_expanded(false);
        c.add_action(SemanticsAction::Expand, noop());
        c.add_action(SemanticsAction::Collapse, noop());
        c.add_action(SemanticsAction::Click, noop());
    });
    f.add(Some(root), 8, |c| {
        c.set_label("Row");
        c.add_action(SemanticsAction::ScrollIntoView, noop());
    });
    f.add(Some(root), 9, |c| {
        c.set_role(SemanticsRole::TextInput);
        c.set_role(SemanticsRole::PasswordInput);
        c.set_value("hunter2");
        c.add_action(SemanticsAction::SetValue, noop());
    });
    f.add(Some(root), 10, |c| {
        c.set_role(SemanticsRole::Slider);
        c.set_numeric_range(crate::NumericRange::new(2.5, 0.0, 10.0, 0.5).expect("finite range"));
        c.add_action(SemanticsAction::SetNumericValue, noop());
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

    let slider = f.only(e(10));
    assert_eq!(
        slider.value.as_deref(),
        Some("2.5"),
        "a numeric node reads its number as the desktop RangeValue does"
    );
    assert_eq!(slider.actions, [N::SetValue]);
}

/// Which handlers a disclosure fixture registers.
#[derive(Clone, Copy)]
enum Disclosure {
    /// Discrete `Expand` and `Collapse` handlers beside a tap handler.
    Explicit,
    /// Only a tap handler, as `Semantics::new().expanded(..).on_tap(..)`
    /// builds; the transition its state allows reaches that handler.
    TapOnly,
}

/// Drives one disclosure shape and state through the wire path
/// (`resolve_wire_action`) and the platform path (`semantics_action_request_for`),
/// both ending in `SemanticsOwner::resolve_action`. The node advertises and
/// accepts only the transition its state allows, and the request reaches the
/// discrete handler, or the tap handler for a tap-only node. The reverse
/// direction is refused on the wire and by the owner, for discrete handlers
/// and a tap-only node's tap handler alike.
fn disclosure_case(shape: Disclosure, expanded: bool) {
    let received = Arc::new(Mutex::new(Vec::new()));
    let mut f = Fixture::new();
    let root = f.add(None, 1, |_| {});
    let log = Arc::clone(&received);
    f.add(Some(root), 2, move |c| {
        c.set_role(SemanticsRole::Button);
        c.set_expanded(expanded);
        let handler = || -> crate::SemanticsActionHandler {
            let log = Arc::clone(&log);
            Arc::new(move |call: ActionCall| log.lock().expect("log").push(call.action()))
        };
        c.add_action(SemanticsAction::Click, handler());
        if matches!(shape, Disclosure::Explicit) {
            c.add_action(SemanticsAction::Expand, handler());
            c.add_action(SemanticsAction::Collapse, handler());
        }
    });
    let (allowed, refused, ak_allowed, ak_refused) = if expanded {
        (
            ActionName::Collapse,
            ActionName::Expand,
            accesskit::Action::Collapse,
            accesskit::Action::Expand,
        )
    } else {
        (
            ActionName::Expand,
            ActionName::Collapse,
            accesskit::Action::Expand,
            accesskit::Action::Collapse,
        )
    };
    let discrete = if expanded {
        SemanticsAction::Collapse
    } else {
        SemanticsAction::Expand
    };
    let reaches = match shape {
        Disclosure::Explicit => discrete,
        Disclosure::TapOnly => SemanticsAction::Click,
    };

    assert_eq!(f.only(e(2)).actions, [allowed], "advertised transition");
    assert_eq!(
        f.owner
            .resolve_wire_action(&ActionRequest::new(e(2), refused)),
        Err(WireActionError::ActionUnsupported {
            element: e(2),
            action: refused
        })
    );
    let wire = f
        .owner
        .resolve_wire_action(&ActionRequest::new(e(2), allowed))
        .expect("the wire accepts the transition the state allows");
    assert_eq!(wire.action(), discrete);
    f.owner
        .resolve_action(wire)
        .expect("the owner routes the wire request")
        .invoke();

    let platform = |action| accesskit::ActionRequest {
        action,
        target_tree: accesskit::TreeId::ROOT,
        target_node: accesskit::NodeId(e(2).get()),
        data: None,
    };
    let translated = crate::semantics_action_request_for(&platform(ak_allowed))
        .expect("the platform transition is routable");
    f.owner
        .resolve_action(translated)
        .expect("the owner routes the platform request")
        .invoke();
    assert_eq!(*received.lock().expect("log"), [reaches, reaches]);

    let reverse = crate::semantics_action_request_for(&platform(ak_refused))
        .expect("the reverse transition is routable");
    assert!(matches!(
        f.owner.resolve_action(reverse),
        Err(crate::SemanticsActionError::UnsupportedAction { .. })
    ));
    assert_eq!(
        *received.lock().expect("log"),
        [reaches, reaches],
        "a refused transition reaches no handler"
    );
}

fn explicit_collapsed() {
    disclosure_case(Disclosure::Explicit, false);
}

fn explicit_expanded() {
    disclosure_case(Disclosure::Explicit, true);
}

fn tap_only_collapsed() {
    disclosure_case(Disclosure::TapOnly, false);
}

fn tap_only_expanded() {
    disclosure_case(Disclosure::TapOnly, true);
}

/// Expand and collapse reach a node only toward the state it lacks, through
/// its discrete handlers or, for a tap-only node, its tap handler.
#[test]
fn disclosure_requests_follow_the_expanded_state() {
    let rows: &[(&str, fn())] = &[
        ("explicit_collapsed", explicit_collapsed),
        ("explicit_expanded", explicit_expanded),
        ("tap_only_collapsed", tap_only_collapsed),
        ("tap_only_expanded", tap_only_expanded),
    ];
    let failed: Vec<&str> = rows
        .iter()
        .filter(|(_, row)| std::panic::catch_unwind(row).is_err())
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "disclosure rows failed: {failed:?}");
}

#[test]
fn set_value_reaches_the_handler_with_its_payload() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&received);
    let mut f = Fixture::new();
    let root = f.add(None, 1, |_| {});
    f.add(Some(root), 2, move |c| {
        c.set_role(SemanticsRole::TextInput);
        c.add_action(
            SemanticsAction::SetValue,
            Arc::new(move |call| {
                sink.lock()
                    .expect("the test lock is not poisoned")
                    .push(call);
            }),
        );
    });

    let request = f
        .owner
        .resolve_wire_action(&ActionRequest::set_value(e(2), "hello"))
        .expect("the field advertises set_value");
    assert_eq!(request.call, ActionCall::SetValue("hello".into()));
    f.owner
        .resolve_action(request)
        .expect("the resolved request reaches the node's handler")
        .invoke();
    assert_eq!(
        *received.lock().expect("the test lock is not poisoned"),
        [ActionCall::SetValue("hello".into())]
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
    assert_eq!(request.call, ActionCall::SetNumericValue(2.375));
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

/// What one node publishes for `set_value`: a text value, a numeric range,
/// and which handlers it registers.
struct ValueShape {
    text: Option<&'static str>,
    range: bool,
    set_text: bool,
    set_number: bool,
}

/// Publishes `shape`, then checks the wire value it reads, whether
/// `set_value` is advertised, and which handler a wire `set_value` of `"7"`
/// runs, with what. The desktop backend writes through the `Value` pattern
/// ahead of `RangeValue`, so `expected` is the handler it would run too.
fn value_case(shape: &ValueShape, value: Option<&str>, expected: Option<SemanticsAction>) {
    let received = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&received);
    let handler = move || -> crate::SemanticsActionHandler {
        let log = Arc::clone(&log);
        Arc::new(move |call| log.lock().expect("log").push(call))
    };
    let mut f = Fixture::new();
    let root = f.add(None, 1, |_| {});
    f.add(Some(root), 2, |c| {
        // A role keeps the node from being lifted as a generic container.
        if shape.range {
            c.set_role(SemanticsRole::Slider);
        } else {
            c.set_role(SemanticsRole::TextInput);
        }
        if let Some(text) = shape.text {
            c.set_value(text);
        }
        if shape.range {
            c.set_numeric_range(
                crate::NumericRange::new(5.0, 0.0, 10.0, 1.0).expect("finite fixture"),
            );
        }
        if shape.set_text {
            c.add_action(SemanticsAction::SetValue, handler());
        }
        if shape.set_number {
            c.add_action(SemanticsAction::SetNumericValue, handler());
        }
    });
    let node = f.only(e(2));
    assert_eq!(node.value.as_deref(), value, "wire value");
    assert_eq!(
        node.actions.contains(&ActionName::SetValue),
        expected.is_some(),
        "set_value advertised"
    );
    let request = f
        .owner
        .resolve_wire_action(&ActionRequest::set_value(e(2), "7"));
    let Some(expected) = expected else {
        assert!(
            matches!(request, Err(WireActionError::ActionUnsupported { .. })),
            "{request:?}"
        );
        return;
    };
    let request = request.expect("an advertised set_value resolves");
    f.owner
        .resolve_action(request)
        .expect("the owner resolves an advertised set_value to a handler")
        .invoke();
    let call = match expected {
        SemanticsAction::SetNumericValue => ActionCall::SetNumericValue(7.0),
        _ => ActionCall::SetValue("7".into()),
    };
    assert_eq!(
        *received.lock().expect("log"),
        [call],
        "the handler that ran"
    );
}

fn numeric_handler_without_range() {
    let shape = ValueShape {
        text: Some("0"),
        range: false,
        set_text: false,
        set_number: true,
    };
    value_case(&shape, Some("0"), None);
}

fn numeric_handler_with_range() {
    let shape = ValueShape {
        text: None,
        range: true,
        set_text: false,
        set_number: true,
    };
    value_case(&shape, Some("5"), Some(SemanticsAction::SetNumericValue));
}

fn text_only() {
    let shape = ValueShape {
        text: Some("fixed"),
        range: false,
        set_text: true,
        set_number: false,
    };
    value_case(&shape, Some("fixed"), Some(SemanticsAction::SetValue));
}

fn text_and_numeric() {
    let shape = ValueShape {
        text: Some("50%"),
        range: true,
        set_text: true,
        set_number: true,
    };
    value_case(&shape, Some("50%"), Some(SemanticsAction::SetValue));
}

fn text_and_numeric_with_only_a_numeric_handler() {
    let shape = ValueShape {
        text: Some("50%"),
        range: true,
        set_text: false,
        set_number: true,
    };
    value_case(&shape, Some("50%"), Some(SemanticsAction::SetNumericValue));
}

/// `set_value` is advertised only where a request can reach a handler. A
/// node with both a text value and a number is written as text, as the
/// desktop backend's `Value`-before-`RangeValue` precedence does, and that
/// text reaches the numeric handler of a range without a text handler.
#[test]
fn set_value_follows_the_value_pattern_precedence() {
    let rows: &[(&str, fn())] = &[
        (
            "numeric_handler_without_range",
            numeric_handler_without_range,
        ),
        ("numeric_handler_with_range", numeric_handler_with_range),
        ("text_only", text_only),
        ("text_and_numeric", text_and_numeric),
        (
            "text_and_numeric_with_only_a_numeric_handler",
            text_and_numeric_with_only_a_numeric_handler,
        ),
    ];
    let failed: Vec<&str> = rows
        .iter()
        .filter(|(_, row)| std::panic::catch_unwind(row).is_err())
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "set_value rows failed: {failed:?}");
}

#[test]
fn a_disabled_node_refuses_with_disabled() {
    let mut f = Fixture::new();
    let root = f.add(None, 1, |_| {});
    f.add(Some(root), 2, |c| {
        c.set_role(SemanticsRole::Button);
        c.set_enabled(Some(false));
        c.add_action(SemanticsAction::Click, noop());
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
        c.set_role(SemanticsRole::Button);
        c.set_label("Save");
        c.add_action(SemanticsAction::Click, noop());
    });
    f.owner
        .get_mut(button)
        .expect("the button was just added")
        .set_rect(LogicalRect::from_ltrb(10.0, 20.25, 40.5, 41.0));

    let tree = f.read(&ReadQuery::new());
    let json = serde_json::to_value(&tree).expect("a tree serializes");
    assert_eq!(json["protocol"], "0.3");
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
