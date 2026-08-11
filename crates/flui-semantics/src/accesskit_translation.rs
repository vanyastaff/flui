//! Translation from FLUI's semantics tree into the AccessKit tree model.
//!
//! AccessKit is the shape both OS accessibility adapters and query-by-role UI
//! testing consume, so this translation is deliberately the *only* place the two
//! diverge from each other: whatever a screen reader is told, a test can assert.
//!
//! # Role is carried in two places
//!
//! FLUI mirrors Flutter, which encodes role at two granularities:
//!
//! - [`SemanticsFlag`] identifies the common controls — `IsButton`, `IsLink`,
//!   `IsTextField`, `IsSlider`. These leave [`SemanticsRole::None`].
//! - [`SemanticsRole`] carries the structural roles a screen reader navigates by
//!   — `Tab`, `Table`, `ColumnHeader`, `MenuItemRadio`. These have no flag.
//!
//! [`resolve_role`] therefore consults both, explicit role first. Reading only
//! the enum would map every button to [`Role::Unknown`]; reading only the flags
//! would lose every structural role.
//!
//! # Full updates, not diffs
//!
//! `TreeUpdate` documents that an update "should only include nodes that are new
//! or changed". FLUI's assembly is a classic full rebuild (ADR-0014), so every
//! pass yields every node and this emits all of them. Platform adapters suppress
//! extraneous events, so that is correct but not free. Incremental diffing is a
//! later optimisation and needs its own oracle; it is not smuggled in here.

use accesskit::{Node, NodeId, Rect, Role, TextDirection, Toggled, Tree, TreeId, TreeUpdate};

use crate::action::SemanticsAction;
use crate::flags::SemanticsFlag;
use crate::owner::SemanticsNodeUpdate;
use crate::role::SemanticsRole;
use crate::update::SemanticsNodeData;

/// Whether `bits` carries `flag`.
#[inline]
fn has_flag(bits: u64, flag: SemanticsFlag) -> bool {
    bits & (flag as u64) != 0
}

/// Whether `bits` carries `action`.
#[inline]
fn has_action(bits: u64, action: SemanticsAction) -> bool {
    bits & (action as u64) != 0
}

/// The AccessKit role for one node.
///
/// An explicit [`SemanticsRole`] wins. Otherwise the role-bearing flags are
/// consulted in specificity order, and a node that claims none of them becomes
/// [`Role::GenericContainer`] — present in the tree and navigable, but making no
/// claim about what it is.
#[must_use]
pub fn resolve_role(data: &SemanticsNodeData) -> Role {
    if let Some(role) = explicit_role(data.role) {
        return role;
    }

    let flags = data.flags;
    if has_flag(flags, SemanticsFlag::IsButton) {
        Role::Button
    } else if has_flag(flags, SemanticsFlag::IsLink) {
        Role::Link
    } else if has_flag(flags, SemanticsFlag::IsTextField) {
        // Multiline is a distinct AccessKit role rather than a property, and
        // screen readers announce the two differently.
        if has_flag(flags, SemanticsFlag::IsMultiline) {
            Role::MultilineTextInput
        } else if has_flag(flags, SemanticsFlag::IsObscured) {
            Role::PasswordInput
        } else {
            Role::TextInput
        }
    } else if has_flag(flags, SemanticsFlag::IsSlider) {
        Role::Slider
    } else if has_flag(flags, SemanticsFlag::IsKeyboardKey) {
        Role::Keyboard
    } else if has_flag(flags, SemanticsFlag::IsImage) {
        Role::Image
    } else if has_flag(flags, SemanticsFlag::IsHeader) {
        Role::Header
    } else if has_flag(flags, SemanticsFlag::HasToggledState) {
        Role::Switch
    } else if has_flag(flags, SemanticsFlag::HasCheckedState) {
        // A checkable inside a mutually-exclusive group is a radio, not a
        // checkbox — the distinction changes how a screen reader reads the set.
        if has_flag(flags, SemanticsFlag::IsInMutuallyExclusiveGroup) {
            Role::RadioButton
        } else {
            Role::CheckBox
        }
    } else {
        Role::GenericContainer
    }
}

/// The AccessKit role for an explicit [`SemanticsRole`], or `None` when the node
/// declares no role and the flags must decide.
fn explicit_role(role: SemanticsRole) -> Option<Role> {
    Some(match role {
        SemanticsRole::None => return None,
        SemanticsRole::AlertDialog => Role::AlertDialog,
        SemanticsRole::Dialog => Role::Dialog,
        SemanticsRole::Tab => Role::Tab,
        SemanticsRole::TabBar => Role::TabList,
        SemanticsRole::TabPanel => Role::TabPanel,
        SemanticsRole::Table => Role::Table,
        SemanticsRole::Cell => Role::Cell,
        SemanticsRole::Row => Role::Row,
        SemanticsRole::ColumnHeader => Role::ColumnHeader,
        SemanticsRole::RadioGroup => Role::RadioGroup,
        SemanticsRole::Menu => Role::Menu,
        SemanticsRole::MenuBar => Role::MenuBar,
        SemanticsRole::MenuItem => Role::MenuItem,
        SemanticsRole::MenuItemCheckbox => Role::MenuItemCheckBox,
        SemanticsRole::MenuItemRadio => Role::MenuItemRadio,
        SemanticsRole::Alert => Role::Alert,
        SemanticsRole::Status => Role::Status,
        SemanticsRole::List => Role::List,
        SemanticsRole::ListItem => Role::ListItem,
        SemanticsRole::Complementary => Role::Complementary,
        SemanticsRole::ContentInfo => Role::ContentInfo,
        SemanticsRole::Main => Role::Main,
        SemanticsRole::Navigation => Role::Navigation,
        SemanticsRole::Region => Role::Region,
        SemanticsRole::Form => Role::Form,
        SemanticsRole::SpinButton => Role::SpinButton,
        SemanticsRole::ComboBox => Role::ComboBox,
        SemanticsRole::Tooltip => Role::Tooltip,
        // AccessKit models both as a progress indicator; it draws no
        // determinate/indeterminate distinction at the role level.
        SemanticsRole::LoadingSpinner | SemanticsRole::ProgressBar => Role::ProgressIndicator,
        // No AccessKit counterpart. Deliberately generic rather than
        // approximated into a role that would mislead a screen reader about
        // what the control does.
        SemanticsRole::DragHandle | SemanticsRole::HotKey => Role::GenericContainer,
    })
}

/// Translate one node's boolean state onto the AccessKit node.
fn apply_state(node: &mut Node, flags: u64) {
    if has_flag(flags, SemanticsFlag::HasCheckedState) {
        node.set_toggled(if has_flag(flags, SemanticsFlag::IsCheckStateMixed) {
            Toggled::Mixed
        } else if has_flag(flags, SemanticsFlag::IsChecked) {
            Toggled::True
        } else {
            Toggled::False
        });
    } else if has_flag(flags, SemanticsFlag::HasToggledState) {
        node.set_toggled(if has_flag(flags, SemanticsFlag::IsToggled) {
            Toggled::True
        } else {
            Toggled::False
        });
    }

    if has_flag(flags, SemanticsFlag::IsSelected) {
        node.set_selected(true);
    }
    if has_flag(flags, SemanticsFlag::HasExpandedState) {
        node.set_expanded(has_flag(flags, SemanticsFlag::IsExpanded));
    }
    // `IsEnabled` only means anything alongside `HasEnabledState`; a node
    // without the state bit is not "disabled", it simply has no such concept.
    if has_flag(flags, SemanticsFlag::HasEnabledState) && !has_flag(flags, SemanticsFlag::IsEnabled)
    {
        node.set_disabled();
    }
    if has_flag(flags, SemanticsFlag::IsReadOnly) {
        node.set_read_only();
    }
    if has_flag(flags, SemanticsFlag::IsHidden) {
        node.set_hidden();
    }
    if has_flag(flags, SemanticsFlag::IsLiveRegion) {
        node.set_live(accesskit::Live::Polite);
    }
}

/// Translate the supported actions onto the AccessKit node.
///
/// Several FLUI actions have no AccessKit counterpart and are intentionally not
/// emitted: the character- and word-wise cursor moves, and `Copy`/`Cut`/`Paste`
/// (AccessKit expects those to reach the app through the platform's own text
/// interface, not as tree actions). `Dismiss` likewise has no equivalent. They
/// are dropped rather than approximated, so nothing claims support it lacks.
fn apply_actions(node: &mut Node, actions: u64) {
    if has_action(actions, SemanticsAction::Tap) {
        node.add_action(accesskit::Action::Click);
    }
    if has_action(actions, SemanticsAction::LongPress) {
        node.add_action(accesskit::Action::ShowContextMenu);
    }
    if has_action(actions, SemanticsAction::ScrollLeft) {
        node.add_action(accesskit::Action::ScrollLeft);
    }
    if has_action(actions, SemanticsAction::ScrollRight) {
        node.add_action(accesskit::Action::ScrollRight);
    }
    if has_action(actions, SemanticsAction::ScrollUp) {
        node.add_action(accesskit::Action::ScrollUp);
    }
    if has_action(actions, SemanticsAction::ScrollDown) {
        node.add_action(accesskit::Action::ScrollDown);
    }
    if has_action(actions, SemanticsAction::Increase) {
        node.add_action(accesskit::Action::Increment);
    }
    if has_action(actions, SemanticsAction::Decrease) {
        node.add_action(accesskit::Action::Decrement);
    }
    if has_action(actions, SemanticsAction::ShowOnScreen) {
        node.add_action(accesskit::Action::ScrollIntoView);
    }
    if has_action(actions, SemanticsAction::SetSelection) {
        node.add_action(accesskit::Action::SetTextSelection);
    }
    if has_action(actions, SemanticsAction::SetText) {
        node.add_action(accesskit::Action::SetValue);
    }
    if has_action(actions, SemanticsAction::ScrollToOffset) {
        node.add_action(accesskit::Action::SetScrollOffset);
    }
    if has_action(actions, SemanticsAction::Focus)
        || has_action(actions, SemanticsAction::DidGainAccessibilityFocus)
    {
        node.add_action(accesskit::Action::Focus);
    }
    if has_action(actions, SemanticsAction::DidLoseAccessibilityFocus) {
        node.add_action(accesskit::Action::Blur);
    }
    if has_action(actions, SemanticsAction::CustomAction) {
        node.add_action(accesskit::Action::CustomAction);
    }
}

/// Translate one FLUI semantics node into an AccessKit node.
#[must_use]
pub fn to_node(data: &SemanticsNodeData) -> Node {
    let mut node = Node::new(resolve_role(data));

    if let Some(label) = &data.label {
        node.set_label(label.as_str());
    }
    if let Some(value) = &data.value {
        node.set_value(value.as_str());
    }
    // FLUI's `hint` is supplementary prose about what a control does, which is
    // what AccessKit calls a description.
    if let Some(hint) = &data.hint {
        node.set_description(hint.as_str());
    }
    if let Some(tooltip) = &data.tooltip {
        node.set_tooltip(tooltip.as_str());
    }
    if let Some(direction) = data.text_direction {
        node.set_text_direction(match direction {
            crate::properties::TextDirection::Ltr => TextDirection::LeftToRight,
            crate::properties::TextDirection::Rtl => TextDirection::RightToLeft,
        });
    }

    node.set_bounds(Rect {
        x0: f64::from(data.rect.left().0),
        y0: f64::from(data.rect.top().0),
        x1: f64::from(data.rect.right().0),
        y1: f64::from(data.rect.bottom().0),
    });

    if let Some(position) = data.scroll_position {
        node.set_scroll_y(position);
    }
    if let Some(max) = data.scroll_extent_max {
        node.set_scroll_y_max(max);
    }
    if let Some(min) = data.scroll_extent_min {
        node.set_scroll_y_min(min);
    }

    apply_state(&mut node, data.flags);
    apply_actions(&mut node, data.actions);

    node.set_children(
        data.children
            .iter()
            .copied()
            .map(NodeId)
            .collect::<Vec<_>>(),
    );

    node
}

/// Translate a batch of FLUI semantics updates into one AccessKit
/// [`TreeUpdate`].
///
/// The root is the update whose `parent` is `None`. `focus` names the node the
/// platform should treat as focused; when it is `None`, or names a node not in
/// this batch, focus falls back to the root — AccessKit requires a valid focus
/// target, and pointing it at a node the adapter has never seen is worse than
/// pointing it at the root.
///
/// Returns `None` when `updates` contains no root, since a `TreeUpdate` without
/// one cannot be applied.
#[must_use]
pub fn to_tree_update(
    updates: &[SemanticsNodeUpdate],
    focus: Option<NodeId>,
) -> Option<TreeUpdate> {
    let root = updates
        .iter()
        .find(|update| update.parent.is_none())
        .map(|update| NodeId(update.data.id))?;

    let nodes: Vec<(NodeId, Node)> = updates
        .iter()
        .map(|update| (NodeId(update.data.id), to_node(&update.data)))
        .collect();

    let focus = focus
        .filter(|wanted| nodes.iter().any(|(id, _)| id == wanted))
        .unwrap_or(root);

    Some(TreeUpdate {
        nodes,
        tree: Some(Tree::new(root)),
        tree_id: TreeId::ROOT,
        focus,
    })
}

/// Translate a whole [`SemanticsTree`](crate::tree::SemanticsTree) into one
/// AccessKit [`TreeUpdate`].
///
/// This is the entry point for a consumer holding the assembled tree — the
/// platform bridge publishing after `run_semantics`, or a test harness asking
/// what the frame currently exposes. [`to_tree_update`] is its counterpart for
/// the incremental `SemanticsUpdateCallback` payload.
///
/// Returns `None` for a tree with no root, which cannot produce an applicable
/// update.
#[must_use]
pub fn tree_to_update(
    tree: &crate::tree::SemanticsTree,
    focus: Option<flui_foundation::SemanticsId>,
) -> Option<TreeUpdate> {
    let root = tree.root()?;
    let root_node_id = NodeId((root.get() - 1) as u64);

    let nodes: Vec<(NodeId, Node)> = tree
        .iter()
        .map(|(id, node)| {
            let data = node.to_node_data(id);
            (NodeId(data.id), to_node(&data))
        })
        .collect();

    let focus = focus
        .map(|id| NodeId((id.get() - 1) as u64))
        .filter(|wanted| nodes.iter().any(|(id, _)| id == wanted))
        .unwrap_or(root_node_id);

    Some(TreeUpdate {
        nodes,
        tree: Some(Tree::new(root_node_id)),
        tree_id: TreeId::ROOT,
        focus,
    })
}

#[cfg(test)]
mod tests {
    use flui_foundation::SemanticsId;
    use flui_types::Rect;
    use flui_types::geometry::px;

    use super::*;

    /// Builds an update whose payload id is `id`, with no parent (so it is the
    /// root) unless one is given.
    fn update(id: u64, data: SemanticsNodeData) -> SemanticsNodeUpdate {
        SemanticsNodeUpdate::new(
            SemanticsId::new((id + 1) as usize),
            SemanticsNodeData { id, ..data },
        )
    }

    fn flags(bits: &[SemanticsFlag]) -> u64 {
        bits.iter().fold(0, |acc, f| acc | (*f as u64))
    }

    /// **The case a naive `SemanticsRole` match loses.**
    ///
    /// A button carries `SemanticsRole::None` and is identified purely by the
    /// `IsButton` flag, so translating only the role enum maps the most common
    /// widget in any application to `Role::Unknown` — and every existing
    /// `run_semantics` test still passes, because none of them look at roles.
    #[test]
    fn a_button_declares_no_explicit_role_and_must_still_translate_to_role_button() {
        let data = SemanticsNodeData {
            flags: flags(&[SemanticsFlag::IsButton, SemanticsFlag::IsFocusable]),
            label: Some("Save".into()),
            ..Default::default()
        };
        assert_eq!(data.role, SemanticsRole::None, "premise: no explicit role");

        let node = to_node(&data);

        assert_eq!(node.role(), Role::Button);
        assert_eq!(node.label(), Some("Save"));
    }

    /// The mirror case: a structural role has no flag and lives only in the
    /// enum, so flag-only derivation loses it.
    #[test]
    fn a_structural_role_survives_when_no_flag_could_express_it() {
        let data = SemanticsNodeData {
            role: SemanticsRole::ColumnHeader,
            ..Default::default()
        };
        assert_eq!(to_node(&data).role(), Role::ColumnHeader);
    }

    #[test]
    fn an_explicit_role_wins_over_a_role_bearing_flag() {
        let data = SemanticsNodeData {
            role: SemanticsRole::MenuItem,
            flags: flags(&[SemanticsFlag::IsButton]),
            ..Default::default()
        };
        assert_eq!(
            to_node(&data).role(),
            Role::MenuItem,
            "an explicitly declared role must not be overridden by a flag"
        );
    }

    #[test]
    fn a_checkbox_translates_all_three_of_its_states() {
        let checkable = [SemanticsFlag::HasCheckedState];
        assert_eq!(
            to_node(&SemanticsNodeData {
                flags: flags(&checkable),
                ..Default::default()
            })
            .toggled(),
            Some(Toggled::False)
        );
        assert_eq!(
            to_node(&SemanticsNodeData {
                flags: flags(&[SemanticsFlag::HasCheckedState, SemanticsFlag::IsChecked]),
                ..Default::default()
            })
            .toggled(),
            Some(Toggled::True)
        );
        assert_eq!(
            to_node(&SemanticsNodeData {
                flags: flags(&[
                    SemanticsFlag::HasCheckedState,
                    SemanticsFlag::IsCheckStateMixed
                ]),
                ..Default::default()
            })
            .toggled(),
            Some(Toggled::Mixed),
            "tristate must not collapse to checked/unchecked"
        );
    }

    /// A checkable inside a mutually-exclusive group is a radio button, and a
    /// screen reader reads the two differently.
    #[test]
    fn a_checkable_in_a_mutually_exclusive_group_is_a_radio_button() {
        let data = SemanticsNodeData {
            flags: flags(&[
                SemanticsFlag::HasCheckedState,
                SemanticsFlag::IsInMutuallyExclusiveGroup,
            ]),
            ..Default::default()
        };
        assert_eq!(to_node(&data).role(), Role::RadioButton);
    }

    #[test]
    fn an_obscured_text_field_is_a_password_input_and_a_multiline_one_is_distinct() {
        let obscured = SemanticsNodeData {
            flags: flags(&[SemanticsFlag::IsTextField, SemanticsFlag::IsObscured]),
            ..Default::default()
        };
        assert_eq!(to_node(&obscured).role(), Role::PasswordInput);

        let multiline = SemanticsNodeData {
            flags: flags(&[SemanticsFlag::IsTextField, SemanticsFlag::IsMultiline]),
            ..Default::default()
        };
        assert_eq!(to_node(&multiline).role(), Role::MultilineTextInput);
    }

    /// `IsEnabled` is only meaningful alongside `HasEnabledState`. A node with
    /// neither is not disabled — it has no such concept — and marking it
    /// disabled would make a screen reader announce every plain container as
    /// unavailable.
    #[test]
    fn a_node_without_enabled_state_is_not_reported_disabled() {
        assert!(!to_node(&SemanticsNodeData::default()).is_disabled());

        let disabled = SemanticsNodeData {
            flags: flags(&[SemanticsFlag::HasEnabledState]),
            ..Default::default()
        };
        assert!(to_node(&disabled).is_disabled());

        let enabled = SemanticsNodeData {
            flags: flags(&[SemanticsFlag::HasEnabledState, SemanticsFlag::IsEnabled]),
            ..Default::default()
        };
        assert!(!to_node(&enabled).is_disabled());
    }

    #[test]
    fn actions_translate_to_their_accesskit_counterparts() {
        let data = SemanticsNodeData {
            actions: (SemanticsAction::Tap as u64)
                | (SemanticsAction::Increase as u64)
                | (SemanticsAction::ScrollDown as u64),
            ..Default::default()
        };
        let node = to_node(&data);
        assert!(node.supports_action(accesskit::Action::Click));
        assert!(node.supports_action(accesskit::Action::Increment));
        assert!(node.supports_action(accesskit::Action::ScrollDown));
        assert!(
            !node.supports_action(accesskit::Action::Decrement),
            "an action the node never declared must not appear"
        );
    }

    #[test]
    fn bounds_carry_the_nodes_rect() {
        let data = SemanticsNodeData {
            rect: Rect::from_xywh(px(10.0), px(20.0), px(100.0), px(50.0)),
            ..Default::default()
        };
        let bounds = to_node(&data).bounds().expect("bounds are always set");
        assert!((bounds.x0 - 10.0).abs() < f64::EPSILON);
        assert!((bounds.y0 - 20.0).abs() < f64::EPSILON);
        assert!((bounds.x1 - 110.0).abs() < f64::EPSILON);
        assert!((bounds.y1 - 70.0).abs() < f64::EPSILON);
    }

    #[test]
    fn the_root_is_the_parentless_update_and_children_are_preserved() {
        let root = update(
            0,
            SemanticsNodeData {
                children: [1, 2].into_iter().collect(),
                ..Default::default()
            },
        );
        let first = update(1, SemanticsNodeData::default()).with_parent(Some(SemanticsId::new(1)));
        let second = update(2, SemanticsNodeData::default()).with_parent(Some(SemanticsId::new(1)));

        let tree_update = to_tree_update(&[root, first, second], None).expect("a root is present");

        assert_eq!(tree_update.nodes.len(), 3);
        assert_eq!(
            tree_update.tree.as_ref().expect("tree").root,
            NodeId(0),
            "the parentless update is the root"
        );
        let (_, root_node) = tree_update
            .nodes
            .iter()
            .find(|(id, _)| *id == NodeId(0))
            .expect("root node present");
        assert_eq!(root_node.children(), &[NodeId(1), NodeId(2)]);
    }

    /// AccessKit requires a valid focus target. Naming a node the adapter has
    /// never seen is worse than naming the root, so an out-of-batch focus falls
    /// back rather than being passed through.
    #[test]
    fn focus_falls_back_to_the_root_when_the_named_node_is_not_in_the_batch() {
        let root = update(0, SemanticsNodeData::default());
        let tree_update = to_tree_update(&[root], Some(NodeId(99))).expect("a root is present");
        assert_eq!(tree_update.focus, NodeId(0));
    }

    #[test]
    fn a_named_focus_inside_the_batch_is_honoured() {
        let root = update(
            0,
            SemanticsNodeData {
                children: [1].into_iter().collect(),
                ..Default::default()
            },
        );
        let child = update(1, SemanticsNodeData::default()).with_parent(Some(SemanticsId::new(1)));
        let tree_update =
            to_tree_update(&[root, child], Some(NodeId(1))).expect("a root is present");
        assert_eq!(tree_update.focus, NodeId(1));
    }

    /// A batch with no parentless node cannot produce an applicable update, and
    /// inventing a root would hand the adapter a tree that is not the app's.
    #[test]
    fn a_batch_with_no_root_yields_no_update() {
        let orphan = update(7, SemanticsNodeData::default()).with_parent(Some(SemanticsId::new(1)));
        assert!(to_tree_update(&[orphan], None).is_none());
    }
}

#[cfg(test)]
mod owner_entry_point_tests {
    use flui_types::Rect;
    use flui_types::geometry::px;

    use super::*;
    use crate::node::SemanticsNode;
    use crate::owner::SemanticsOwner;

    /// The owner-level entry point is what a platform bridge and a test harness
    /// both call, so it must produce a tree whose roles are queryable — the
    /// whole point of routing both through one translation.
    #[test]
    fn the_owner_publishes_a_queryable_tree_for_the_assembled_semantics() {
        let mut owner = SemanticsOwner::new_without_callback();

        let mut node = SemanticsNode::new();
        node.set_rect(Rect::from_xywh(px(0.0), px(0.0), px(200.0), px(100.0)));
        let root = owner.tree_mut().insert(node);
        owner.tree_mut().set_root(Some(root));

        let update = owner
            .to_accesskit_tree_update(None)
            .expect("a rooted tree yields an update");

        assert_eq!(
            update.tree.as_ref().expect("tree").root,
            NodeId((root.get() - 1) as u64)
        );
        assert_eq!(update.focus, update.tree.as_ref().expect("tree").root);
        assert_eq!(update.nodes.len(), 1);
    }

    /// Before the first assembly pass there is no root, and inventing one would
    /// hand the adapter a tree the application does not have.
    #[test]
    fn an_unassembled_tree_yields_no_update() {
        let owner = SemanticsOwner::new_without_callback();
        assert!(owner.to_accesskit_tree_update(None).is_none());
    }

    /// A button reaches the published tree as `Role::Button`, through the owner
    /// rather than the raw translation — the path a harness actually uses.
    #[test]
    fn a_button_is_findable_by_role_through_the_owner() {
        let mut owner = SemanticsOwner::new_without_callback();
        let mut node = SemanticsNode::new();
        node.config_mut().set_button(true);
        node.config_mut().set_label("Save");
        let root = owner.tree_mut().insert(node);
        owner.tree_mut().set_root(Some(root));

        let update = owner
            .to_accesskit_tree_update(None)
            .expect("a rooted tree yields an update");

        let button = update
            .nodes
            .iter()
            .find(|(_, node)| node.role() == Role::Button)
            .map(|(_, node)| node)
            .expect("the button must be findable by role in the published tree");
        assert_eq!(button.label(), Some("Save"));
    }
}
