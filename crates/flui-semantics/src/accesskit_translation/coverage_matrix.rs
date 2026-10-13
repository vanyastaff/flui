//! The coverage matrix: every role and every attribute FLUI models, read back
//! through `accesskit_consumer` as an adapter reads it, rather than compared
//! field by field against what translation wrote (R21).
//!
//! A new attribute or state does not compile until this module names it,
//! either with a row or with the reason it is not published.

use accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
use accesskit_consumer::{FilterResult, NodeRef, common_filter};
use smallvec::SmallVec;

use super::{PublishView, to_node};
use crate::attributes::SemanticsAttributes;
use crate::identity::AccessibilityNodeId;
use crate::properties::{HeadingLevel, TextDirection};
use crate::role::{Assertiveness, SemanticsRole};
use crate::states::{States, Toggled};
use crate::{NumericRange, SemanticsAction};

/// The window root every row's node hangs under.
const ROOT: NodeId = NodeId(u64::MAX);

/// Reads one node as an adapter would: published under a window root, with
/// `actions` as its effective action bits and the keyboard focus on it when
/// `focused`.
fn read<R>(
    attrs: &SemanticsAttributes,
    actions: u64,
    focused: bool,
    check: impl FnOnce(&NodeRef<'_>) -> R,
) -> R {
    let id = AccessibilityNodeId::from(flui_foundation::RenderId::new(2));
    let child = NodeId(id.as_u64());
    let node = to_node(&PublishView {
        id,
        attrs,
        actions,
        rect: flui_foundation::geometry::Rect::from_ltwh(0.0, 0.0, 10.0, 10.0),
        children: SmallVec::new(),
    });
    let mut root = Node::new(Role::Window);
    root.set_children(vec![child]);
    let tree = accesskit_consumer::Tree::new(
        TreeUpdate {
            nodes: vec![(ROOT, root), (child, node)],
            tree: Some(TreeInfo::new(ROOT)),
            tree_id: TreeId::ROOT,
            focus: if focused { child } else { ROOT },
        },
        true,
    );
    let state = tree.state();
    let node = state
        .root()
        .children()
        .next()
        .expect("the row's node hangs under the root");
    check(&node)
}

/// Reads a parent node with one child, both published under a window root.
fn read_with_child<R>(
    parent: &SemanticsAttributes,
    child: &SemanticsAttributes,
    check: impl FnOnce(&NodeRef<'_>) -> R,
) -> R {
    let parent_id = AccessibilityNodeId::from(flui_foundation::RenderId::new(2));
    let child_id = AccessibilityNodeId::from(flui_foundation::RenderId::new(3));
    let rect = flui_foundation::geometry::Rect::from_ltwh(0.0, 0.0, 10.0, 10.0);
    let child_node = to_node(&PublishView {
        id: child_id,
        attrs: child,
        actions: 0,
        rect,
        children: SmallVec::new(),
    });
    let parent_node = to_node(&PublishView {
        id: parent_id,
        attrs: parent,
        actions: 0,
        rect,
        children: smallvec::smallvec![child_id],
    });
    let mut root = Node::new(Role::Window);
    root.set_children(vec![NodeId(parent_id.as_u64())]);
    let tree = accesskit_consumer::Tree::new(
        TreeUpdate {
            nodes: vec![
                (ROOT, root),
                (NodeId(parent_id.as_u64()), parent_node),
                (NodeId(child_id.as_u64()), child_node),
            ],
            tree: Some(TreeInfo::new(ROOT)),
            tree_id: TreeId::ROOT,
            focus: ROOT,
        },
        true,
    );
    let state = tree.state();
    let node = state
        .root()
        .children()
        .next()
        .expect("the parent hangs under the root");
    check(&node)
}

/// One attribute, set on an otherwise empty node of `role`, and what the
/// consumer must then report.
struct Row {
    attribute: &'static str,
    role: SemanticsRole,
    set: fn(&mut SemanticsAttributes),
    actions: u64,
    focused: bool,
    reads: fn(&NodeRef<'_>) -> bool,
}

impl Row {
    const fn new(
        attribute: &'static str,
        role: SemanticsRole,
        set: fn(&mut SemanticsAttributes),
        reads: fn(&NodeRef<'_>) -> bool,
    ) -> Self {
        Self {
            attribute,
            role,
            set,
            actions: 0,
            focused: false,
            reads,
        }
    }
}

fn range() -> NumericRange {
    NumericRange::new(4.0, 0.0, 10.0, 2.0).expect("finite fixture")
}

/// Every published attribute reaches the consumer as its AccessKit property.
#[test]
#[expect(
    clippy::redundant_closure_for_method_calls,
    reason = "a method path is not higher-ranked over the node's lifetime, so it cannot be a row's reader"
)]
fn every_published_attribute_reads_through_the_consumer() {
    use SemanticsRole as R;

    // Exhaustive: a new attribute or state fails to compile here until it has
    // a row below or a reason it is not published.
    let SemanticsAttributes {
        states: _,
        live: _,
        label: _,
        value: _,
        numeric_range: _,
        hint: _,
        tooltip: _,
        placeholder: _,
        text_direction: _,
        custom_actions: _,
        scroll_position: _,
        scroll_axis: _,
        scroll_extent_max: _,
        scroll_extent_min: _,
        index_in_parent: _,
        scroll_child_count: _,
        heading_level: _,
        role: _,
        // Resolved into `labelled_by` and the other relations by
        // `to_published_node`; read through the consumer by
        // `relations_resolve_through_anchors_across_publication`.
        anchors: _,
        relations: _,
        // Framework-side: merge bookkeeping and traversal order.
        tags: _,
        sort_key: _,
        hint_overrides: _,
        scroll_index: _,
        // No AccessKit property yet: adjacent values, platform views and text
        // lengths join with the changes that route them.
        increased_value: _,
        decreased_value: _,
        platform_view_id: _,
        max_value_length: _,
        current_value_length: _,
    } = SemanticsAttributes::default();
    let States {
        toggled: _,
        selected: _,
        expanded: _,
        enabled: _,
        read_only: _,
        hidden: _,
        modal: _,
        required: _,
        invalid: _,
        // Not per-node properties: AccessKit expresses focusability as the
        // `Focus` action a focusable node registers, and focus as the update's
        // `focus`, which the owner resolves from `focused`
        // (`an_unattached_focus_claim_does_not_steal_focus`).
        focusable: _,
        focused: _,
        // Framework-side: route scoping and naming.
        scopes_route: _,
        names_route: _,
    } = States::default();

    let rows = [
        Row::new(
            "label",
            R::Button,
            |a| a.label = Some("Send".into()),
            |n| n.label().as_deref() == Some("Send"),
        ),
        Row::new(
            "label of static text",
            R::None,
            |a| a.label = Some("Hello".into()),
            |n| n.label_comes_from_value() && n.value().as_deref() == Some("Hello"),
        ),
        Row::new(
            "label of a role ARIA forbids a name",
            R::Paragraph,
            |a| {
                a.label = Some("Terms apply".into());
                a.hint = Some("Read before paying".into());
            },
            |n| {
                n.label().is_none()
                    && n.description().as_deref() == Some("Terms apply\nRead before paying")
            },
        ),
        Row::new(
            "value",
            R::TextInput,
            |a| a.value = Some("draft".into()),
            |n| n.value().as_deref() == Some("draft"),
        ),
        Row::new(
            "numeric range",
            R::Slider,
            |a| a.numeric_range = Some(range()),
            |n| {
                n.numeric_value() == Some(4.0)
                    && n.min_numeric_value() == Some(0.0)
                    && n.max_numeric_value() == Some(10.0)
                    && n.numeric_value_step() == Some(2.0)
            },
        ),
        Row::new(
            "hint",
            R::Button,
            |a| a.hint = Some("Sends the message".into()),
            |n| n.description().as_deref() == Some("Sends the message"),
        ),
        Row::new(
            "tooltip",
            R::Button,
            |a| a.tooltip = Some("Send".into()),
            |n| n.data().tooltip() == Some("Send"),
        ),
        Row::new(
            "placeholder",
            R::TextInput,
            |a| a.placeholder = Some("Search".into()),
            |n| n.placeholder() == Some("Search"),
        ),
        Row::new(
            "text direction",
            R::Paragraph,
            |a| a.text_direction = Some(TextDirection::Rtl),
            |n| n.data().text_direction() == Some(accesskit::TextDirection::RightToLeft),
        ),
        Row {
            actions: SemanticsAction::CustomAction.value(),
            ..Row::new(
                "custom actions",
                R::ListItem,
                |a| a.custom_actions = smallvec::smallvec!["Archive".into()],
                |n| {
                    n.data()
                        .custom_actions()
                        .iter()
                        .any(|action| action.description == "Archive")
                },
            )
        },
        Row::new(
            "vertical scroll",
            R::ScrollView,
            |a| {
                a.scroll_position = Some(20.0);
                a.scroll_extent_min = Some(0.0);
                a.scroll_extent_max = Some(100.0);
            },
            |n| {
                n.scroll_y() == Some(20.0)
                    && n.scroll_y_min() == Some(0.0)
                    && n.scroll_y_max() == Some(100.0)
            },
        ),
        Row::new(
            "horizontal scroll",
            R::ScrollView,
            |a| {
                a.scroll_axis = Some(flui_foundation::geometry::Axis::Horizontal);
                a.scroll_position = Some(20.0);
                a.scroll_extent_max = Some(100.0);
            },
            |n| {
                n.scroll_x() == Some(20.0)
                    && n.scroll_x_max() == Some(100.0)
                    && n.scroll_y().is_none()
            },
        ),
        Row::new(
            "place in set",
            R::ListItem,
            |a| {
                a.index_in_parent = Some(2);
                a.scroll_child_count = Some(9);
            },
            // Zero-based, as AccessKit documents; the adapters add one. The count
            // is published on the item, while adapters read it from the
            // container (`size_of_set_from_container`): the item's own value is
            // what this row can pin until the count moves to the container.
            |n| n.position_in_set() == Some(2) && n.size_of_set() == Some(9),
        ),
        Row::new(
            "heading level",
            R::Heading,
            |a| a.heading_level = Some(HeadingLevel::H3),
            |n| n.level() == Some(2),
        ),
        Row::new(
            "live",
            R::None,
            |a| {
                a.label = Some("Saved".into());
                a.live = Some(Assertiveness::Assertive);
            },
            |n| n.live() == accesskit::Live::Assertive,
        ),
        Row::new(
            "toggled",
            R::CheckBox,
            |a| a.states.toggled = Some(Toggled::Mixed),
            |n| n.toggled() == Some(accesskit::Toggled::Mixed),
        ),
        Row::new(
            "selected",
            R::Tab,
            |a| a.states.selected = Some(true),
            |n| n.is_selected() == Some(true),
        ),
        Row::new(
            "declared unselected",
            R::Tab,
            |a| a.states.selected = Some(false),
            // A declared `false` keeps the node selectable to adapters, which a
            // node with no selection state is not.
            |n| n.is_selected() == Some(false) && n.is_selectable(),
        ),
        Row::new(
            "expanded",
            R::ComboBox,
            |a| a.states.expanded = Some(false),
            |n| n.data().is_expanded() == Some(false),
        ),
        Row::new(
            "disabled",
            R::Button,
            |a| a.states.enabled = Some(false),
            |n| n.is_disabled(),
        ),
        Row::new(
            "read only",
            R::TextInput,
            |a| a.states.read_only = Some(true),
            |n| n.is_read_only(),
        ),
        Row::new(
            "hidden",
            R::Button,
            |a| a.states.hidden = Some(true),
            |n| n.is_hidden() && common_filter(n) == FilterResult::ExcludeSubtree,
        ),
        Row::new(
            "modal",
            R::Dialog,
            |a| a.states.modal = Some(true),
            |n| n.is_modal(),
        ),
        Row::new(
            "required",
            R::TextInput,
            |a| a.states.required = Some(true),
            |n| n.is_required(),
        ),
        Row::new(
            "invalid",
            R::TextInput,
            |a| a.states.invalid = Some(true),
            |n| n.data().invalid() == Some(accesskit::Invalid::True),
        ),
        Row {
            actions: SemanticsAction::Focus.value(),
            ..Row::new(
                "focus action",
                R::Button,
                |_| {},
                |n| n.is_focusable(&common_filter),
            )
        },
        Row {
            focused: true,
            ..Row::new(
                "update focus",
                R::Button,
                |_| {},
                |n| n.is_focused_in_tree(),
            )
        },
    ];

    let mut failed = Vec::new();
    for row in &rows {
        let mut attrs = SemanticsAttributes {
            role: row.role,
            ..SemanticsAttributes::default()
        };
        (row.set)(&mut attrs);
        if !read(&attrs, row.actions, row.focused, |node| (row.reads)(node)) {
            failed.push(row.attribute);
        }
    }
    assert!(
        failed.is_empty(),
        "the consumer does not read these attributes as published: {failed:?}"
    );
}

/// An attribute the declared role does not support is not published (R3):
/// the consumer reads no trace of it.
#[test]
fn an_inapplicable_attribute_is_not_published() {
    use SemanticsRole as R;
    let rows = [
        Row::new(
            "selected on a button",
            R::Button,
            |a| a.states.selected = Some(true),
            |n| n.is_selected().is_none(),
        ),
        Row::new(
            "checked on a link",
            R::Link,
            |a| a.states.toggled = Some(Toggled::On),
            |n| n.toggled().is_none(),
        ),
        Row::new(
            "expanded on a paragraph",
            R::Paragraph,
            |a| a.states.expanded = Some(true),
            |n| n.data().is_expanded().is_none(),
        ),
        Row::new(
            "read only on a button",
            R::Button,
            |a| a.states.read_only = Some(true),
            // The consumer reports every role without read-only support as
            // read-only, so the published flag itself is read.
            |n| !n.data().is_read_only(),
        ),
        Row::new(
            "required on a slider",
            R::Slider,
            |a| a.states.required = Some(true),
            |n| !n.is_required(),
        ),
        Row::new(
            "modal on a button",
            R::Button,
            |a| a.states.modal = Some(true),
            |n| !n.is_modal(),
        ),
        Row::new(
            "a range on a button",
            R::Button,
            |a| a.numeric_range = Some(range()),
            |n| n.numeric_value().is_none(),
        ),
        Row::new(
            "a placeholder on a button",
            R::Button,
            |a| a.placeholder = Some("Search".into()),
            |n| n.placeholder().is_none(),
        ),
        Row::new(
            "a place in set on a button",
            R::Button,
            |a| {
                a.index_in_parent = Some(1);
                a.scroll_child_count = Some(4);
            },
            |n| n.position_in_set().is_none() && n.size_of_set().is_none(),
        ),
    ];
    let mut published = Vec::new();
    for row in &rows {
        let mut attrs = SemanticsAttributes {
            role: row.role,
            ..SemanticsAttributes::default()
        };
        (row.set)(&mut attrs);
        if !read(&attrs, row.actions, row.focused, |node| (row.reads)(node)) {
            published.push(row.attribute);
        }
    }
    assert!(
        published.is_empty(),
        "published although the role does not support it: {published:?}"
    );
}

/// Every role is a node an adapter keeps and names under the AccessKit role
/// ADR-0203 maps it to: a labelled node of any role but `None` passes the
/// consumer's filter as exactly that role and reads its label as its name,
/// or, on a role ARIA forbids a name, as its description.
#[test]
fn every_role_is_kept_and_named_through_the_consumer() {
    use SemanticsRole as R;
    use accesskit::Role as A;
    // Transcribed from ADR-0203's mapping and checked against it, so a role
    // swapped for another (two published as each other) fails here.
    let expected: &[(R, A)] = &[
        (R::Button, A::Button),
        (R::CheckBox, A::CheckBox),
        (R::RadioButton, A::RadioButton),
        (R::Switch, A::Switch),
        (R::Link, A::Link),
        (R::Slider, A::Slider),
        (R::SpinButton, A::SpinButton),
        (R::ComboBox, A::ComboBox),
        (R::ScrollBar, A::ScrollBar),
        (R::Separator, A::Splitter),
        (R::TextInput, A::TextInput),
        (R::MultilineTextInput, A::MultilineTextInput),
        (R::PasswordInput, A::PasswordInput),
        (R::SearchInput, A::SearchInput),
        (R::EmailInput, A::EmailInput),
        (R::NumberInput, A::NumberInput),
        (R::PhoneNumberInput, A::PhoneNumberInput),
        (R::UrlInput, A::UrlInput),
        (R::DateInput, A::DateInput),
        (R::TimeInput, A::TimeInput),
        (R::ListBox, A::ListBox),
        (R::ListBoxOption, A::ListBoxOption),
        (R::RadioGroup, A::RadioGroup),
        (R::Menu, A::Menu),
        (R::MenuBar, A::MenuBar),
        (R::MenuItem, A::MenuItem),
        (R::MenuItemCheckbox, A::MenuItemCheckBox),
        (R::MenuItemRadio, A::MenuItemRadio),
        (R::Tab, A::Tab),
        (R::TabList, A::TabList),
        (R::TabPanel, A::TabPanel),
        (R::Tree, A::Tree),
        (R::TreeItem, A::TreeItem),
        (R::TreeGrid, A::TreeGrid),
        (R::Grid, A::Grid),
        (R::GridCell, A::GridCell),
        (R::Toolbar, A::Toolbar),
        (R::Table, A::Table),
        (R::Row, A::Row),
        (R::RowGroup, A::RowGroup),
        (R::Cell, A::Cell),
        (R::ColumnHeader, A::ColumnHeader),
        (R::RowHeader, A::RowHeader),
        (R::List, A::List),
        (R::ListItem, A::ListItem),
        (R::Heading, A::Heading),
        (R::Paragraph, A::Paragraph),
        (R::Image, A::Image),
        (R::Figure, A::Figure),
        (R::Caption, A::Caption),
        (R::Article, A::Article),
        (R::Feed, A::Feed),
        (R::Group, A::Group),
        (R::Note, A::Note),
        (R::Blockquote, A::Blockquote),
        (R::Code, A::Code),
        (R::Emphasis, A::Emphasis),
        (R::Strong, A::Strong),
        (R::Insertion, A::ContentInsertion),
        (R::Deletion, A::ContentDeletion),
        (R::Term, A::Term),
        (R::Definition, A::Definition),
        (R::Time, A::Time),
        (R::Math, A::Math),
        (R::Meter, A::Meter),
        (R::ProgressBar, A::ProgressIndicator),
        (R::Tooltip, A::Tooltip),
        (R::Document, A::Document),
        (R::Application, A::Application),
        (R::Banner, A::Banner),
        (R::Main, A::Main),
        (R::Navigation, A::Navigation),
        (R::Complementary, A::Complementary),
        (R::ContentInfo, A::ContentInfo),
        (R::Search, A::Search),
        (R::Form, A::Form),
        (R::Region, A::Region),
        (R::Alert, A::Alert),
        (R::Status, A::Status),
        (R::Log, A::Log),
        (R::Marquee, A::Marquee),
        (R::Timer, A::Timer),
        (R::Dialog, A::Dialog),
        (R::AlertDialog, A::AlertDialog),
        (R::Window, A::Window),
        (R::Pane, A::Pane),
        (R::ScrollView, A::ScrollView),
    ];
    let mut failed = Vec::new();
    for &role in SemanticsRole::ALL {
        if role == SemanticsRole::None {
            continue;
        }
        let Some(&(_, want)) = expected.iter().find(|(known, _)| *known == role) else {
            failed.push(format!("{role}: no expected AccessKit role in the table"));
            continue;
        };
        let attrs = SemanticsAttributes {
            role,
            label: Some("Name".into()),
            ..SemanticsAttributes::default()
        };
        let kept = read(&attrs, 0, false, |node| {
            let included = common_filter(node) == FilterResult::Include;
            // A role ARIA forbids a name carries its text as the description.
            let named = if crate::applicability::prohibits_name(role) {
                node.label().is_none() && node.description().as_deref() == Some("Name")
            } else {
                node.label().as_deref() == Some("Name")
            };
            (included && named && node.role() == want)
                .then_some(())
                .ok_or_else(|| {
                    format!(
                        "{role}: role {:?} (want {want:?}), filter {:?}, name {:?}",
                        node.role(),
                        common_filter(node),
                        node.label()
                    )
                })
        });
        if let Err(row) = kept {
            failed.push(row);
        }
    }
    assert!(
        failed.is_empty(),
        "roles an adapter would not keep or name as mapped:\n{}",
        failed.join("\n")
    );
}

/// The AccessKit releases this matrix was checked against. Bumping either
/// fails [`the_matrix_was_checked_against_the_locked_accesskit`] until the
/// rows are re-read against the new release and these move with it (R21).
const MATRIX_CHECKED_AGAINST: [(&str, &str); 2] =
    [("accesskit", "0.25.1"), ("accesskit_consumer", "0.39.1")];

/// The versions of `package` the workspace's `Cargo.lock` holds.
pub(crate) fn locked_versions(package: &str) -> Vec<String> {
    let lock_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock");
    let lock = std::fs::read_to_string(lock_path)
        .unwrap_or_else(|error| panic!("reading {lock_path}: {error}"));
    let name = format!(r#"name = "{package}""#);
    lock.split("[[package]]")
        .filter(|entry| entry.lines().any(|line| line.trim() == name))
        .filter_map(|entry| {
            entry.lines().find_map(|line| {
                line.trim()
                    .strip_prefix("version = \"")
                    .and_then(|rest| rest.strip_suffix('"'))
                    .map(str::to_owned)
            })
        })
        .collect()
}

#[test]
fn the_matrix_was_checked_against_the_locked_accesskit() {
    for (package, checked) in MATRIX_CHECKED_AGAINST {
        assert_eq!(
            locked_versions(package),
            [checked],
            "the workspace locks a different {package}: re-read the coverage matrix's rows \
             against it, then update MATRIX_CHECKED_AGAINST"
        );
    }
}

/// A control the consumer names from its content (AccName's name from
/// descendants, R5) takes its static-text child's text as its name when it
/// has no label of its own.
#[test]
#[expect(
    clippy::redundant_closure_for_method_calls,
    reason = "a method path is not higher-ranked over the node's lifetime, so it cannot be the reader"
)]
fn a_control_without_a_label_is_named_by_its_content() {
    use SemanticsRole as R;
    let text = SemanticsAttributes {
        label: Some("Send".into()),
        ..SemanticsAttributes::default()
    };
    let mut unnamed = Vec::new();
    for role in [
        R::Button,
        R::Link,
        R::CheckBox,
        R::RadioButton,
        R::MenuItem,
        R::MenuItemCheckbox,
        R::MenuItemRadio,
    ] {
        let control = SemanticsAttributes {
            role,
            ..SemanticsAttributes::default()
        };
        if read_with_child(&control, &text, |node| node.label()).as_deref() != Some("Send") {
            unnamed.push(role);
        }
    }
    assert!(
        unnamed.is_empty(),
        "not named by their content: {unnamed:?}"
    );
}
