//! The coverage matrix: every role and every attribute FLUI models, read back
//! through `accesskit_consumer` as an adapter reads it, rather than compared
//! field by field against what translation wrote.
//!
//! A new attribute or state does not compile until this module names it,
//! either with a row or with the reason it is not published.

use std::num::{NonZeroU32, NonZeroUsize};

use accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
use accesskit_consumer::{FilterResult, NodeRef, common_filter};
use smallvec::SmallVec;

use super::{PublishView, to_node};
use crate::attributes::SemanticsAttributes;
use crate::identity::AccessibilityNodeId;
use crate::properties::{HeadingLevel, TextDirection};
use crate::role::{LiveSetting, SemanticsRole};
use crate::states::{AutoComplete, Current, HasPopup, Invalid, States, Toggled};
use crate::{CellPosition, NumericRange, SemanticsAction, SetSize, SortDirection, TableShape};
use flui_foundation::geometry::Axis;

/// The window root every row's node hangs under.
const ROOT: NodeId = NodeId(u64::MAX);

/// Reads a node as an adapter would: published under a window root with
/// `children` below it, `actions` as its effective action bits, and the
/// keyboard focus on it when `focused`.
fn read_tree<R>(
    attrs: &SemanticsAttributes,
    children: &[&SemanticsAttributes],
    actions: u64,
    focused: bool,
    check: impl FnOnce(&NodeRef<'_>) -> R,
) -> R {
    let rect = flui_foundation::geometry::Rect::from_ltwh(0.0, 0.0, 10.0, 10.0);
    let id = AccessibilityNodeId::from(flui_foundation::RenderId::new(2));
    let child_ids: SmallVec<[AccessibilityNodeId; 4]> = (0..children.len())
        .map(|index| AccessibilityNodeId::from(flui_foundation::RenderId::new(3 + index)))
        .collect();
    let mut nodes = vec![(
        NodeId(id.as_u64()),
        to_node(&PublishView {
            id,
            attrs,
            actions,
            rect,
            children: child_ids.clone(),
            text_selection: None,
        }),
    )];
    for (&child, child_id) in children.iter().zip(&child_ids) {
        nodes.push((
            NodeId(child_id.as_u64()),
            to_node(&PublishView {
                id: *child_id,
                attrs: child,
                actions: 0,
                rect,
                children: SmallVec::new(),
                text_selection: None,
            }),
        ));
    }
    let mut root = Node::new(Role::Window);
    root.set_children(vec![NodeId(id.as_u64())]);
    nodes.insert(0, (ROOT, root));
    let tree = accesskit_consumer::Tree::new(
        TreeUpdate {
            nodes,
            tree: Some(TreeInfo::new(ROOT)),
            tree_id: TreeId::ROOT,
            focus: if focused { NodeId(id.as_u64()) } else { ROOT },
        },
        true,
    );
    let state = tree.state();
    let node = state
        .root()
        .children()
        .next()
        .expect("the node hangs under the root");
    check(&node)
}

/// Reads one childless node; see [`read_tree`].
fn read<R>(
    attrs: &SemanticsAttributes,
    actions: u64,
    focused: bool,
    check: impl FnOnce(&NodeRef<'_>) -> R,
) -> R {
    read_tree(attrs, &[], actions, focused, check)
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

/// The rows whose node the consumer does not read as the row expects.
fn failing_rows(rows: &[Row]) -> Vec<&'static str> {
    rows.iter()
        .filter(|row| {
            let mut attrs = SemanticsAttributes {
                role: row.role,
                ..SemanticsAttributes::default()
            };
            (row.set)(&mut attrs);
            !read(&attrs, row.actions, row.focused, |node| (row.reads)(node))
        })
        .map(|row| row.attribute)
        .collect()
}

/// A one-based row, column or span.
fn nz(number: usize) -> NonZeroUsize {
    NonZeroUsize::new(number).expect("BUG: a fixture number is not zero")
}

fn range() -> NumericRange {
    NumericRange::try_new(4.0, 0.0, 10.0, 2.0).expect("finite fixture")
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
        live_atomic: _,
        label: _,
        value: _,
        numeric_range: _,
        hint: _,
        tooltip: _,
        placeholder: _,
        keyboard_shortcut: _,
        language: _,
        url: _,
        role_description: _,
        identifier: _,
        text_direction: _,
        custom_actions: _,
        scroll_position: _,
        scroll_axis: _,
        scroll_extent_max: _,
        scroll_extent_min: _,
        collection: _,
        heading_level: _,
        role: _,
        // Resolved into `labelled_by` and the other relations by
        // `to_published_node`: the resolved ids are pinned by
        // `relations_resolve_through_anchors_across_publication`, and the name a
        // `labelled_by` target gives, read through the consumer, by
        // `a_labelled_by_target_names_its_source_through_the_consumer`.
        anchors: _,
        relations: _,
        // Framework-side: merge bookkeeping and traversal order.
        tags: _,
        sort_key: _,
        hint_overrides: _,
        scroll_index: _,
        // Unpublished: AccessKit has no adjacent-value, platform-view or
        // text-length property.
        increased_value: _,
        decreased_value: _,
        platform_view_id: _,
        max_value_length: _,
        current_value_length: _,
        // Published as `TextRun` children, read through the consumer by
        // `tests/text_runs.rs`.
        text: _,
        // Published as the text selection of a document, in the runs of the
        // texts beneath it; read through the consumer by the widget rows of
        // ADR-0214.
        document_selection: _,
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
        busy: _,
        invalid: _,
        current: _,
        has_popup: _,
        auto_complete: _,
        orientation: _,
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
            "identifier",
            R::Button,
            |a| a.identifier = Some("send-button".into()),
            // The adapters' `AutomationId`, read by no screen reader: the node
            // stays unnamed and undescribed.
            |n| {
                n.author_id() == Some("send-button")
                    && n.label().is_none()
                    && n.description().is_none()
                    && n.value().is_none()
            },
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
            "a terminal place in set is dropped",
            R::ListItem,
            |a| a.collection.position_in_set = Some(i32::MAX as usize),
            // The adapters add one as an `i32`, which `i32::MAX` cannot take.
            |n| n.position_in_set().is_none(),
        ),
        Row::new(
            "place in set",
            R::ListItem,
            |a| a.collection.position_in_set = Some(2),
            // Zero-based, as AccessKit documents; the adapters add one. The
            // size is the container's: `a_member_reads_its_size_from_its_container`.
            |n| n.position_in_set() == Some(2) && n.size_of_set().is_none(),
        ),
        Row::new(
            "size of the set a node contains",
            R::List,
            |a| a.collection.set_size = Some(SetSize::Known(9)),
            |n| n.size_of_set() == Some(9),
        ),
        Row::new(
            "an unknown size of set",
            R::List,
            |a| a.collection.set_size = Some(SetSize::Unknown),
            // AccessKit has no "unknown"; a number would claim the set ends
            // where the built members do.
            |n| n.size_of_set().is_none(),
        ),
        Row::new(
            "level of a tree item",
            R::TreeItem,
            |a| a.collection.level = NonZeroU32::new(3),
            // One-based for the author, zero-based in AccessKit.
            |n| n.level() == Some(2),
        ),
        Row::new(
            "rows and columns of a table",
            R::Table,
            |a| {
                a.collection.table = Some(TableShape::new(SetSize::Known(40), SetSize::Known(5)));
            },
            |n| n.data().row_count() == Some(40) && n.data().column_count() == Some(5),
        ),
        Row::new(
            "an unknown row count",
            R::Grid,
            |a| {
                a.collection.table = Some(TableShape::new(SetSize::Unknown, SetSize::Known(5)));
            },
            |n| n.data().row_count().is_none() && n.data().column_count() == Some(5),
        ),
        Row::new(
            "a cell's place in its table",
            R::Cell,
            |a| a.collection.cell = Some(CellPosition::new(nz(5), nz(2))),
            // One-based for the author, zero-based in AccessKit, one span of
            // each unless the cell says otherwise.
            |n| {
                n.data().row_index() == Some(4)
                    && n.data().column_index() == Some(1)
                    && n.data().row_span().is_none()
                    && n.data().column_span().is_none()
            },
        ),
        Row::new(
            "a row's own index",
            R::Row,
            |a| a.collection.row = Some(nz(7)),
            |n| n.data().row_index() == Some(6) && n.data().column_index().is_none(),
        ),
        Row::new(
            "a level too deep for the adapters",
            R::TreeItem,
            |a| a.collection.level = NonZeroU32::new(u32::MAX),
            // The adapters add one and convert to `i32`: a level past it
            // would read as no level at all, so it is not published.
            |n| n.level().is_none(),
        ),
        Row::new(
            "a cell that spans",
            R::GridCell,
            |a| {
                a.collection.cell = Some(CellPosition::new(nz(1), nz(1)).spanning(nz(2), nz(3)));
            },
            |n| n.data().row_span() == Some(2) && n.data().column_span() == Some(3),
        ),
        Row::new(
            "a cell whose span the adapters cannot convert",
            R::GridCell,
            |a| {
                let huge = NonZeroUsize::new(usize::MAX).expect("usize::MAX is non-zero");
                a.collection.cell = Some(CellPosition::new(nz(2), nz(2)).spanning(huge, nz(1)));
            },
            // An omitted span reads as one, which would misplace the cell: it
            // is not published at all.
            |n| {
                n.data().row_index().is_none()
                    && n.data().column_index().is_none()
                    && n.data().row_span().is_none()
            },
        ),
        Row::new(
            "the sort of a column",
            R::ColumnHeader,
            |a| a.collection.sort = Some(SortDirection::Descending),
            |n| n.sort_direction() == Some(accesskit::SortDirection::Descending),
        ),
        Row::new(
            "a container that selects many",
            R::ListBox,
            |a| a.collection.multiselectable = Some(true),
            |n| n.is_multiselectable(),
        ),
        Row::new(
            "a container that selects one",
            R::ListBox,
            |a| a.collection.multiselectable = Some(false),
            |n| !n.is_multiselectable(),
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
                a.live = Some(LiveSetting::Assertive);
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
            "busy",
            R::Log,
            |a| a.states.busy = Some(true),
            |n| n.data().is_busy(),
        ),
        Row::new(
            "live atomic",
            R::None,
            |a| {
                a.label = Some("3 results".into());
                a.live = Some(LiveSetting::Polite);
                a.live_atomic = Some(true);
            },
            |n| n.live() == accesskit::Live::Polite && n.is_live_atomic(),
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
            |a| a.states.invalid = Some(Invalid::True),
            |n| n.data().invalid() == Some(accesskit::Invalid::True),
        ),
        Row::new(
            "invalid spelling",
            R::TextInput,
            |a| a.states.invalid = Some(Invalid::Spelling),
            |n| n.data().invalid() == Some(accesskit::Invalid::Spelling),
        ),
        Row::new(
            "invalid grammar",
            R::TextInput,
            |a| a.states.invalid = Some(Invalid::Grammar),
            |n| n.data().invalid() == Some(accesskit::Invalid::Grammar),
        ),
        Row::new(
            "validated and passed",
            R::TextInput,
            |a| a.states.invalid = Some(Invalid::False),
            |n| n.data().invalid().is_none(),
        ),
        Row::new(
            "current page",
            R::Link,
            |a| a.states.current = Some(Current::Page),
            |n| n.data().aria_current() == Some(accesskit::AriaCurrent::Page),
        ),
        Row::new(
            "current step",
            R::ListItem,
            |a| a.states.current = Some(Current::Step),
            |n| n.data().aria_current() == Some(accesskit::AriaCurrent::Step),
        ),
        Row::new(
            "has popup",
            R::Button,
            |a| a.states.has_popup = Some(HasPopup::ListBox),
            |n| n.data().has_popup() == Some(accesskit::HasPopup::Listbox),
        ),
        Row::new(
            "auto complete",
            R::ComboBox,
            |a| a.states.auto_complete = Some(AutoComplete::Both),
            |n| n.data().auto_complete() == Some(accesskit::AutoComplete::Both),
        ),
        Row::new(
            "orientation",
            R::Slider,
            |a| a.states.orientation = Some(Axis::Vertical),
            |n| n.data().orientation() == Some(accesskit::Orientation::Vertical),
        ),
        Row::new(
            "horizontal orientation",
            R::TabList,
            |a| a.states.orientation = Some(Axis::Horizontal),
            |n| n.data().orientation() == Some(accesskit::Orientation::Horizontal),
        ),
        Row::new(
            "keyboard shortcut",
            R::Button,
            |a| a.keyboard_shortcut = Some("Control+S".into()),
            |n| n.data().keyboard_shortcut() == Some("Control+S"),
        ),
        Row::new(
            "language",
            R::None,
            |a| {
                a.label = Some("Bonjour".into());
                a.language = Some("fr-FR".into());
            },
            |n| n.data().language() == Some("fr-FR"),
        ),
        Row::new(
            "url",
            R::Link,
            |a| {
                a.label = Some("Docs".into());
                a.url = Some("https://example.org/docs".into());
            },
            |n| n.data().url() == Some("https://example.org/docs"),
        ),
        Row::new(
            "role description",
            R::Group,
            |a| a.role_description = Some("slide".into()),
            |n| n.data().role_description() == Some("slide"),
        ),
        Row::new(
            "large step",
            R::Slider,
            |a| {
                a.numeric_range = Some(range().with_jump(5.0).expect("positive jump"));
            },
            |n| n.data().numeric_value_jump() == Some(5.0),
        ),
        Row::new(
            "generic",
            R::Generic,
            |a| a.label = Some("Wrapper".into()),
            |n| {
                n.role() == Role::GenericContainer
                    && n.label().is_none()
                    && n.description().as_deref() == Some("Wrapper")
            },
        ),
        Row::new(
            "presentation",
            R::Presentation,
            |a| a.label = Some("Decoration".into()),
            |n| {
                n.role() == Role::GenericContainer
                    && !n.label_comes_from_value()
                    && n.value().is_none()
            },
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

    let failed = failing_rows(&rows);
    assert!(
        failed.is_empty(),
        "the consumer does not read these attributes as published: {failed:?}"
    );
}

/// An attribute the declared role does not support is not published:
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
            "orientation on a button",
            R::Button,
            |a| a.states.orientation = Some(Axis::Horizontal),
            |n| n.data().orientation().is_none(),
        ),
        Row::new(
            "a popup on a heading",
            R::Heading,
            |a| a.states.has_popup = Some(HasPopup::Menu),
            |n| n.data().has_popup().is_none(),
        ),
        Row::new(
            "auto complete on a button",
            R::Button,
            |a| a.states.auto_complete = Some(AutoComplete::List),
            |n| n.data().auto_complete().is_none(),
        ),
        Row::new(
            "a table shape on a set container with no role",
            R::None,
            |a| {
                a.collection.set_size = Some(SetSize::Known(3));
                a.collection.table = Some(TableShape::new(SetSize::Known(3), SetSize::Known(3)));
            },
            // The node is published as the list it stands for, so it is held
            // to what ARIA gives a list: no table, however it was declared.
            |n| n.size_of_set() == Some(3) && n.data().row_count().is_none(),
        ),
        Row::new(
            "a level on a button",
            R::Button,
            |a| a.collection.level = NonZeroU32::new(2),
            |n| n.level().is_none(),
        ),
        Row::new(
            "a table shape on a list",
            R::List,
            |a| {
                a.collection.table = Some(TableShape::new(SetSize::Known(3), SetSize::Known(3)));
            },
            |n| n.data().row_count().is_none() && n.data().column_count().is_none(),
        ),
        Row::new(
            "a cell position on a list item",
            R::ListItem,
            |a| a.collection.cell = Some(CellPosition::new(nz(1), nz(1))),
            |n| n.data().row_index().is_none() && n.data().column_index().is_none(),
        ),
        Row::new(
            "a column and spans on a row",
            R::Row,
            |a| a.collection.cell = Some(CellPosition::new(nz(1), nz(2))),
            |n| n.data().row_index().is_none() && n.data().column_index().is_none(),
        ),
        Row::new(
            "a sort on a cell",
            R::Cell,
            |a| a.collection.sort = Some(SortDirection::Ascending),
            |n| n.sort_direction().is_none(),
        ),
        Row::new(
            "multiselectable on a button",
            R::Button,
            |a| a.collection.multiselectable = Some(true),
            |n| !n.is_multiselectable(),
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
            "read only on a checkable menu item",
            R::MenuItemCheckbox,
            |a| a.states.read_only = Some(true),
            |n| !n.data().is_read_only(),
        ),
        Row::new(
            "a heading level on a table",
            R::Table,
            |a| a.heading_level = Some(HeadingLevel::H2),
            |n| n.level().is_none(),
        ),
    ];
    let published = failing_rows(&rows);
    assert!(
        published.is_empty(),
        "published although the role does not support it: {published:?}"
    );
}

/// "Item 3 of 9" is read from two nodes: the position from the item and the
/// size from its container, which the consumer finds by walking up through the
/// ancestors it keeps (`size_of_set_from_container`). A container that
/// declares no role is a generic container, which the consumer skips, so it
/// publishes as a list.
#[test]
fn a_member_reads_its_size_from_its_container() {
    let member = |position: usize| SemanticsAttributes {
        role: SemanticsRole::ListItem,
        collection: crate::collection::Collection {
            position_in_set: Some(position),
            ..Default::default()
        },
        ..SemanticsAttributes::default()
    };
    let members = [member(0), member(1), member(2)];
    let members: Vec<&SemanticsAttributes> = members.iter().collect();

    for (container_role, published) in [
        (SemanticsRole::ListBox, Role::ListBox),
        (SemanticsRole::None, Role::List),
    ] {
        let container = SemanticsAttributes {
            role: container_role,
            collection: crate::collection::Collection {
                set_size: Some(SetSize::Known(9)),
                ..Default::default()
            },
            ..SemanticsAttributes::default()
        };
        let read = read_tree(&container, &members, 0, false, |node| {
            (
                node.role(),
                node.children()
                    .map(|child| {
                        (
                            child.position_in_set(),
                            child.size_of_set_from_container(&common_filter),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        });
        assert_eq!(
            read,
            (
                published,
                vec![(Some(0), Some(9)), (Some(1), Some(9)), (Some(2), Some(9))]
            ),
            "a {container_role:?} container"
        );
    }
}

/// Every role is a node an adapter keeps and names under the AccessKit role
/// ADR-0203 maps it to: a labelled node of any role but `None` passes the
/// consumer's filter as exactly that role and reads its label as its name,
/// or, on a role ARIA forbids a name, as its description.
#[test]
fn every_role_is_kept_and_named_through_the_consumer() {
    use SemanticsRole as R;
    use accesskit::Role as A;
    // WAI-ARIA 1.2's "Name From: prohibited" roles, transcribed here rather
    // than read from the predicate publication uses, so a role dropped from
    // it fails.
    const NAME_PROHIBITED: [R; 7] = [
        R::Caption,
        R::Code,
        R::Deletion,
        R::Emphasis,
        R::Insertion,
        R::Paragraph,
        R::Strong,
    ];
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
        // Structure only: published as a container the consumer lifts its
        // children out of (the `generic` and `presentation` rows of
        // `every_published_attribute_reads_through_the_consumer`).
        if matches!(role, R::None | R::Generic | R::Presentation) {
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
            let named = if NAME_PROHIBITED.contains(&role) {
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
/// rows are re-read against the new release and these move with it.
const MATRIX_CHECKED_AGAINST: [(&str, &str); 2] =
    [("accesskit", "0.25.2"), ("accesskit_consumer", "0.39.2")];

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
/// descendants) takes its static-text child's text as its name when it
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
        if read_tree(&control, &[&text], 0, false, |node| node.label()).as_deref() != Some("Send") {
            unnamed.push(role);
        }
    }
    assert!(
        unnamed.is_empty(),
        "not named by their content: {unnamed:?}"
    );
}
