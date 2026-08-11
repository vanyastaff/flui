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
use crate::node::SemanticsNode;
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
pub(crate) fn resolve_role(data: &SemanticsNodeData) -> Role {
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
///
/// `children` are the caller's already-resolved AccessKit ids. They are NOT
/// derived from [`SemanticsNodeData::children`], which holds arena positions in
/// the `SemanticsId` space — see [`tree_to_update`] for why that space must not
/// reach an adapter.
#[must_use]
pub(crate) fn to_node(data: &SemanticsNodeData, children: Vec<NodeId>) -> Node {
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

    node.set_children(children);

    node
}

/// Translate a whole [`SemanticsTree`](crate::tree::SemanticsTree) into one
/// AccessKit [`TreeUpdate`].
///
/// This is the entry point for a consumer holding the assembled tree — the
/// platform bridge publishing after `run_semantics`, or a test harness asking
/// what the frame currently exposes.
///
/// # Node identity
///
/// AccessKit ids come from [`SemanticsNode::accessibility_id`], **not** from
/// `SemanticsId`. The distinction is load-bearing in both directions:
///
/// - `SemanticsId` is an arena position in a tree the pipeline rebuilds every
///   pass, so it is not stable across frames. Exporting it would move a
///   control's identity whenever a sibling was inserted or removed, dropping
///   screen-reader focus, and would let a recycled slot silently re-use a
///   retired control's id.
/// - Actions come *back* addressed by
///   [`AccessibilityNodeId`](crate::identity::AccessibilityNodeId), which
///   [`SemanticsOwner::resolve_action`](crate::owner::SemanticsOwner::resolve_action)
///   matches against `accessibility_id()`. Publishing a tree keyed on anything
///   else means every action an assistive technology sends fails to resolve.
///
/// The stable identity follows the generational [`RenderId`](flui_foundation::RenderId)
/// of the boundary's render object, so configuration changes and sibling
/// reordering preserve it while slot reuse mints a fresh one.
///
/// # Unaddressable nodes
///
/// A node with no source render object has no OS-facing identity and is
/// skipped, along with the parent's reference to it. Every node the pipeline
/// assembles carries one, so this is unreachable in production; a
/// hand-constructed node is dropped rather than exported under a fabricated id
/// that could collide with a real one.
///
/// Returns `None` for a tree whose root is missing or unaddressable, which
/// cannot produce an applicable update.
#[must_use]
pub fn tree_to_update(
    tree: &crate::tree::SemanticsTree,
    focus: Option<flui_foundation::SemanticsId>,
) -> Option<TreeUpdate> {
    let stable_id = |id: flui_foundation::SemanticsId| -> Option<NodeId> {
        tree.get(id)
            .and_then(SemanticsNode::accessibility_id)
            .map(|accessibility_id| NodeId(accessibility_id.as_u64()))
    };

    let root_node_id = stable_id(tree.root()?)?;

    let nodes: Vec<(NodeId, Node)> = tree
        .iter()
        .filter_map(|(id, node)| {
            let node_id = stable_id(id)?;
            let children = node
                .children()
                .iter()
                .filter_map(|&child| stable_id(child))
                .collect();
            Some((node_id, to_node(&node.to_node_data(id), children)))
        })
        .collect();

    let focus = focus
        .and_then(stable_id)
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
    use crate::identity::AccessibilityNodeId;
    use crate::tree::SemanticsTree;

    /// Childless translation; child wiring is covered by the tree-level tests.
    fn translate(data: &SemanticsNodeData) -> Node {
        to_node(data, Vec::new())
    }

    /// A render identity whose packed value is deliberately unequal to any
    /// plausible arena position, so a test cannot pass by coincidence.
    fn render_id(index: u32) -> flui_foundation::RenderId {
        flui_foundation::RenderId::new_gen(
            index,
            core::num::NonZeroU32::new(7).expect("fixture generation is non-zero"),
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

        let node = translate(&data);

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
        assert_eq!(translate(&data).role(), Role::ColumnHeader);
    }

    #[test]
    fn an_explicit_role_wins_over_a_role_bearing_flag() {
        let data = SemanticsNodeData {
            role: SemanticsRole::MenuItem,
            flags: flags(&[SemanticsFlag::IsButton]),
            ..Default::default()
        };
        assert_eq!(
            translate(&data).role(),
            Role::MenuItem,
            "an explicitly declared role must not be overridden by a flag"
        );
    }

    #[test]
    fn a_checkbox_translates_all_three_of_its_states() {
        let checkable = [SemanticsFlag::HasCheckedState];
        assert_eq!(
            translate(&SemanticsNodeData {
                flags: flags(&checkable),
                ..Default::default()
            })
            .toggled(),
            Some(Toggled::False)
        );
        assert_eq!(
            translate(&SemanticsNodeData {
                flags: flags(&[SemanticsFlag::HasCheckedState, SemanticsFlag::IsChecked]),
                ..Default::default()
            })
            .toggled(),
            Some(Toggled::True)
        );
        assert_eq!(
            translate(&SemanticsNodeData {
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
        assert_eq!(translate(&data).role(), Role::RadioButton);
    }

    #[test]
    fn an_obscured_text_field_is_a_password_input_and_a_multiline_one_is_distinct() {
        let obscured = SemanticsNodeData {
            flags: flags(&[SemanticsFlag::IsTextField, SemanticsFlag::IsObscured]),
            ..Default::default()
        };
        assert_eq!(translate(&obscured).role(), Role::PasswordInput);

        let multiline = SemanticsNodeData {
            flags: flags(&[SemanticsFlag::IsTextField, SemanticsFlag::IsMultiline]),
            ..Default::default()
        };
        assert_eq!(translate(&multiline).role(), Role::MultilineTextInput);
    }

    /// `IsEnabled` is only meaningful alongside `HasEnabledState`. A node with
    /// neither is not disabled — it has no such concept — and marking it
    /// disabled would make a screen reader announce every plain container as
    /// unavailable.
    #[test]
    fn a_node_without_enabled_state_is_not_reported_disabled() {
        assert!(!translate(&SemanticsNodeData::default()).is_disabled());

        let disabled = SemanticsNodeData {
            flags: flags(&[SemanticsFlag::HasEnabledState]),
            ..Default::default()
        };
        assert!(translate(&disabled).is_disabled());

        let enabled = SemanticsNodeData {
            flags: flags(&[SemanticsFlag::HasEnabledState, SemanticsFlag::IsEnabled]),
            ..Default::default()
        };
        assert!(!translate(&enabled).is_disabled());
    }

    #[test]
    fn actions_translate_to_their_accesskit_counterparts() {
        let data = SemanticsNodeData {
            actions: (SemanticsAction::Tap as u64)
                | (SemanticsAction::Increase as u64)
                | (SemanticsAction::ScrollDown as u64),
            ..Default::default()
        };
        let node = translate(&data);
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
        let bounds = translate(&data).bounds().expect("bounds are always set");
        assert!((bounds.x0 - 10.0).abs() < f64::EPSILON);
        assert!((bounds.y0 - 20.0).abs() < f64::EPSILON);
        assert!((bounds.x1 - 110.0).abs() < f64::EPSILON);
        assert!((bounds.y1 - 70.0).abs() < f64::EPSILON);
    }

    /// **The identity contract.** AccessKit ids must be the stable
    /// `AccessibilityNodeId` (a packed generational `RenderId`), never the
    /// arena position. The arena positions here are 1 and 2; the render
    /// identities are deliberately unrelated numbers, so a translation that
    /// leaked `SemanticsId` would produce visibly different ids.
    #[test]
    fn node_ids_are_the_stable_render_identity_not_the_arena_position() {
        let mut tree = SemanticsTree::new();
        let root_render = render_id(41);
        let child_render = render_id(87);

        let child = tree.insert(SemanticsNode::new().with_source_render_id(child_render));
        let mut root_node = SemanticsNode::new().with_source_render_id(root_render);
        root_node.add_child(child);
        let root = tree.insert(root_node);
        tree.set_root(Some(root));

        let update = tree_to_update(&tree, None).expect("a rooted tree yields an update");

        let expected_root = NodeId(AccessibilityNodeId::from(root_render).as_u64());
        let expected_child = NodeId(AccessibilityNodeId::from(child_render).as_u64());

        assert_eq!(update.tree.as_ref().expect("tree").root, expected_root);
        assert_ne!(
            expected_root,
            NodeId((root.get() - 1) as u64),
            "the fixture is only meaningful while the two id spaces differ"
        );

        let (_, root_node) = update
            .nodes
            .iter()
            .find(|(id, _)| *id == expected_root)
            .expect("root is published under its stable id");
        assert_eq!(
            root_node.children(),
            &[expected_child],
            "child references must be in the same stable space as the ids"
        );
    }

    /// Why the contract matters: an action arrives addressed by
    /// `AccessibilityNodeId`, and `SemanticsOwner::resolve_action` matches it
    /// against `accessibility_id()`. A tree published under any other id space
    /// makes every incoming action unresolvable.
    #[test]
    fn published_ids_are_the_space_actions_come_back_in() {
        let mut tree = SemanticsTree::new();
        let source = render_id(12);
        let root = tree.insert(SemanticsNode::new().with_source_render_id(source));
        tree.set_root(Some(root));

        let update = tree_to_update(&tree, None).expect("a rooted tree yields an update");
        let (published, _) = update.nodes.first().expect("one node");

        let addressable = tree
            .get(root)
            .and_then(SemanticsNode::accessibility_id)
            .expect("a render-backed node is addressable");
        assert_eq!(published.0, addressable.as_u64());
    }

    /// Reordering siblings changes arena positions but must not move a
    /// control's identity — that is what keeps screen-reader focus attached
    /// across a rebuild.
    #[test]
    fn reordering_siblings_preserves_each_identity() {
        let first_render = render_id(5);
        let second_render = render_id(9);

        let ids_for = |order: [flui_foundation::RenderId; 2]| {
            let mut tree = SemanticsTree::new();
            let children: Vec<_> = order
                .iter()
                .map(|&r| tree.insert(SemanticsNode::new().with_source_render_id(r)))
                .collect();
            let mut root_node = SemanticsNode::new().with_source_render_id(render_id(1));
            for child in children {
                root_node.add_child(child);
            }
            let root = tree.insert(root_node);
            tree.set_root(Some(root));

            let update = tree_to_update(&tree, None).expect("rooted");
            let (_, root_node) = update
                .nodes
                .iter()
                .find(|(id, _)| *id == update.tree.as_ref().expect("tree").root)
                .expect("root present");
            let mut ids = root_node.children().to_vec();
            ids.sort_by_key(|id| id.0);
            ids
        };

        assert_eq!(
            ids_for([first_render, second_render]),
            ids_for([second_render, first_render]),
            "the same two controls keep the same two identities regardless of order"
        );
    }

    /// Focus is named in `SemanticsId` by the caller and must be translated,
    /// not passed through — the two spaces are not interchangeable.
    #[test]
    fn a_named_focus_is_translated_into_the_stable_space() {
        let mut tree = SemanticsTree::new();
        let child_render = render_id(64);
        let child = tree.insert(SemanticsNode::new().with_source_render_id(child_render));
        let mut root_node = SemanticsNode::new().with_source_render_id(render_id(2));
        root_node.add_child(child);
        let root = tree.insert(root_node);
        tree.set_root(Some(root));

        let update = tree_to_update(&tree, Some(child)).expect("rooted");
        assert_eq!(
            update.focus,
            NodeId(AccessibilityNodeId::from(child_render).as_u64())
        );
    }

    /// AccessKit requires a valid focus target, so a node the adapter has never
    /// seen falls back to the root rather than being passed through.
    #[test]
    fn focus_falls_back_to_the_root_when_the_named_node_is_absent() {
        let mut tree = SemanticsTree::new();
        let root = tree.insert(SemanticsNode::new().with_source_render_id(render_id(3)));
        tree.set_root(Some(root));

        let absent = SemanticsId::new(99);
        let update = tree_to_update(&tree, Some(absent)).expect("rooted");
        assert_eq!(update.focus, update.tree.as_ref().expect("tree").root);
    }

    /// A node with no render source has no OS-facing identity. Exporting it
    /// under a fabricated id could collide with a real control, so it and the
    /// parent's reference to it are dropped.
    #[test]
    fn a_node_without_a_render_source_is_not_exported() {
        let mut tree = SemanticsTree::new();
        let unaddressable = tree.insert(SemanticsNode::new());
        let mut root_node = SemanticsNode::new().with_source_render_id(render_id(4));
        root_node.add_child(unaddressable);
        let root = tree.insert(root_node);
        tree.set_root(Some(root));

        let update = tree_to_update(&tree, None).expect("the root is addressable");

        assert_eq!(
            update.nodes.len(),
            1,
            "only the addressable node is published"
        );
        let (_, root_node) = update.nodes.first().expect("root");
        assert!(
            root_node.children().is_empty(),
            "the parent must not reference a node that was not published"
        );
    }

    /// An unrooted tree cannot produce an applicable update, and inventing a
    /// root would hand the adapter a tree the application does not have.
    #[test]
    fn an_unrooted_tree_yields_no_update() {
        let tree = SemanticsTree::new();
        assert!(tree_to_update(&tree, None).is_none());
    }
}

#[cfg(test)]
mod owner_entry_point_tests {
    use flui_types::Rect;
    use flui_types::geometry::px;

    use super::*;
    use crate::identity::AccessibilityNodeId;
    use crate::node::SemanticsNode;
    use crate::owner::SemanticsOwner;

    /// Production assembly always attaches the boundary's render object, which
    /// is where the OS-facing identity comes from — see `tree_to_update`.
    fn source() -> flui_foundation::RenderId {
        flui_foundation::RenderId::new_gen(
            21,
            core::num::NonZeroU32::new(2).expect("fixture generation is non-zero"),
        )
    }

    /// The owner-level entry point is what a platform bridge and a test harness
    /// both call, so it must produce a tree whose roles are queryable — the
    /// whole point of routing both through one translation.
    #[test]
    fn the_owner_publishes_a_queryable_tree_for_the_assembled_semantics() {
        let mut owner = SemanticsOwner::new_without_callback();

        let mut node = SemanticsNode::new().with_source_render_id(source());
        node.set_rect(Rect::from_xywh(px(0.0), px(0.0), px(200.0), px(100.0)));
        let root = owner.tree_mut().insert(node);
        owner.tree_mut().set_root(Some(root));

        let update = owner
            .to_accesskit_tree_update(None)
            .expect("a rooted tree yields an update");

        assert_eq!(
            update.tree.as_ref().expect("tree").root,
            NodeId(AccessibilityNodeId::from(source()).as_u64())
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
        let mut node = SemanticsNode::new().with_source_render_id(source());
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
