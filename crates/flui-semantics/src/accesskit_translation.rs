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
//! or changed". FLUI's assembly is a classic full rebuild (flui-semantics ARCHITECTURE.md, semantics assembly), so every
//! pass yields every node and this emits all of them. Platform adapters suppress
//! extraneous events, so that is correct but not free. Incremental diffing is a
//! later optimisation and needs its own oracle; it is not smuggled in here.

use accesskit::{Node, NodeId, Rect, Role, TextDirection, Toggled, TreeId, TreeInfo, TreeUpdate};

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
/// consulted in specificity order; a node that claims none of them but carries
/// a label (a plain `Text`, a labelled `Semantics` wrapper) is a
/// [`Role::Label`] — static text, which is what Flutter's platform bridges
/// publish for the same node (`kStaticText`) — and a node with neither becomes
/// [`Role::GenericContainer`], present in the tree for structure but making no
/// claim about what it is.
///
/// The distinction is load-bearing at the OS boundary: AccessKit's consumer
/// filter (`accesskit_consumer::common_filter`) drops `GenericContainer` nodes
/// from what an assistive technology sees, so a labelled text that resolved to
/// it was invisible to VoiceOver — observed on 2026-09-22 through
/// `cargo xtask device macos-a11y`, where the counter's two `Text`s were absent from the
/// `AXUIElement` tree while its button was present.
#[must_use]
pub(crate) fn resolve_role(data: &SemanticsNodeData) -> Role {
    if let Some(role) = explicit_role(data.role) {
        return role;
    }

    let flags = data.flags;
    // The checkable/toggled states are tested before the broad `IsButton`, because
    // they are strictly more specific: a control that reports a checked or toggled
    // state is a checkable, whatever else it also is. The ordering matters because
    // this cascade picks exactly one role from a union of flags, and an ancestor's
    // flags can land on a descendant's node. `Semantics::new().button(true)` over a
    // `Radio` is that shape, and so is `MergeSemantics` over a `ListTile` carrying a
    // `Radio` — the reference's own `RadioListTile` composition: measured, each
    // merged node resolves `Button` under the old order and `RadioButton` under
    // this one.
    //
    // A bare `ListTile` over a `Radio` is a different shape: `ListTile` and `Radio`
    // both set `HasEnabledState`, `is_compatible_with` treats the overlap as a
    // conflict, so they form *separate* nodes — and because they do not merge, the
    // radio keeps a node of its own, which announces as a radio under either
    // order. This reorder is not load-bearing there; the widget publishing the
    // group flag is. See `crates/flui-semantics/ARCHITECTURE.md`.
    //
    // Only the checkable states moved. The flags left below `IsButton` — link,
    // slider, text field, image, header — carry the same theoretical argument, and
    // the reference's corpus is *not* silent about all of them: it pins `isButton`
    // beside `isTextField` (`test/material/dropdown_menu_test.dart`,
    // `testWidgets('ensure exclude semantics for trailing button')`) and beside
    // `isLink` (`test/widgets/semantics_merge_test.dart`, `testWidgets('LinkUri from
    // child is passed up to the parent when merging nodes')`). FLUI's `Role` is
    // single-valued where the reference publishes a flag set, so `IsButton`
    // outranking them is a pre-existing divergence rather than an absence of
    // coverage — recorded, with its replacement test, in
    // `crates/flui-semantics/ARCHITECTURE.md` mapping decision 2. Reordering the
    // rest is a precedence decision that entry has now had to make explicitly.
    if has_flag(flags, SemanticsFlag::HasToggledState) {
        Role::Switch
    } else if has_flag(flags, SemanticsFlag::HasCheckedState) {
        // A checkable inside a mutually-exclusive group is a radio, not a
        // checkbox — the distinction changes how a screen reader reads the set.
        if has_flag(flags, SemanticsFlag::IsInMutuallyExclusiveGroup) {
            Role::RadioButton
        } else {
            Role::CheckBox
        }
    } else if has_flag(flags, SemanticsFlag::IsButton) {
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
    } else if data.label.as_ref().is_some_and(|label| !label.is_empty()) {
        Role::Label
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

/// Translate an inbound AccessKit action back into the FLUI action space.
///
/// The inverse of `apply_actions`'s table — keep the two adjacent and in
/// agreement, because an action advertised outbound but unroutable inbound is
/// a control assistive technology can see and press but that does nothing.
///
/// `None` for actions FLUI has no counterpart for (never emitted outbound, so
/// nothing advertised them); the caller drops the request with a trace.
///
/// Two deliberate asymmetries against the outbound table:
///
/// - `Focus` maps to [`SemanticsAction::Focus`] only. Outbound, a node
///   registering only the legacy `DidGainAccessibilityFocus` *notification*
///   also advertises `Focus`, but Flutter's engine likewise dispatches the
///   platform's focus request as the `focus` action itself — the legacy
///   handler is a notification hook, not the action's implementation.
/// - `Blur` maps to `DidLoseAccessibilityFocus`, which IS the notification,
///   because that is the only vocabulary FLUI (and Flutter) has for it.
#[must_use]
pub fn semantics_action_for(action: accesskit::Action) -> Option<SemanticsAction> {
    match action {
        accesskit::Action::Click => Some(SemanticsAction::Tap),
        accesskit::Action::ShowContextMenu => Some(SemanticsAction::LongPress),
        accesskit::Action::ScrollLeft => Some(SemanticsAction::ScrollLeft),
        accesskit::Action::ScrollRight => Some(SemanticsAction::ScrollRight),
        accesskit::Action::ScrollUp => Some(SemanticsAction::ScrollUp),
        accesskit::Action::ScrollDown => Some(SemanticsAction::ScrollDown),
        accesskit::Action::Increment => Some(SemanticsAction::Increase),
        accesskit::Action::Decrement => Some(SemanticsAction::Decrease),
        accesskit::Action::ScrollIntoView => Some(SemanticsAction::ShowOnScreen),
        accesskit::Action::SetTextSelection => Some(SemanticsAction::SetSelection),
        accesskit::Action::SetValue => Some(SemanticsAction::SetText),
        accesskit::Action::SetScrollOffset => Some(SemanticsAction::ScrollToOffset),
        accesskit::Action::Focus => Some(SemanticsAction::Focus),
        accesskit::Action::Blur => Some(SemanticsAction::DidLoseAccessibilityFocus),
        accesskit::Action::CustomAction => Some(SemanticsAction::CustomAction),
        _ => None,
    }
}

/// Translate an inbound AccessKit action payload into the FLUI argument space.
///
/// The payload companion to [`semantics_action_for`] — keep the two adjacent,
/// because a payload that silently fails to cross this seam turns a
/// screen-reader edit into a no-op with no diagnostic.
///
/// `target` is the node the enclosing request addresses: a text selection
/// whose positions live on a *different* node (AccessKit expresses
/// cross-run selections; FLUI's `SetSelection` is offsets within one node)
/// is unroutable and returns `None`, as does any payload kind FLUI has no
/// argument shape for (`NumericValue`, `ScrollUnit`, `ScrollHint`,
/// `ScrollToPoint`). The caller routes the action WITHOUT arguments and
/// traces the drop — the action itself is still meaningful argument-free
/// for some handlers, and swallowing the whole request would turn a lossy
/// payload into a dead control.
#[must_use]
pub fn semantics_action_args_for(
    data: &accesskit::ActionData,
    target: NodeId,
) -> Option<crate::ActionArgs> {
    match data {
        accesskit::ActionData::Value(text) => Some(crate::ActionArgs::SetText {
            text: text.to_string(),
        }),
        accesskit::ActionData::CustomAction(action_id) => Some(crate::ActionArgs::CustomAction {
            action_id: *action_id,
        }),
        accesskit::ActionData::SetScrollOffset(point) => Some(crate::ActionArgs::ScrollToOffset {
            x: point.x,
            y: point.y,
        }),
        accesskit::ActionData::SetTextSelection(selection) => {
            if selection.anchor.node != target || selection.focus.node != target {
                return None;
            }
            let base = i32::try_from(selection.anchor.character_index).ok()?;
            let extent = i32::try_from(selection.focus.character_index).ok()?;
            Some(crate::ActionArgs::SetSelection { base, extent })
        }
        _ => None,
    }
}

/// Translate one FLUI semantics node into an AccessKit node.
///
/// Children come from [`SemanticsNodeData::children`], which carries the
/// stable [`AccessibilityNodeId`](crate::AccessibilityNodeId)s — the same
/// space AccessKit ids are published in, so the mapping is a transparent
/// repack. (An earlier payload shape held arena positions there, and this
/// function had to refuse them; the type now makes that leak
/// unrepresentable.)
#[must_use]
pub(crate) fn to_node(data: &SemanticsNodeData) -> Node {
    let role = resolve_role(data);
    let mut node = Node::new(role);

    if let Some(label) = &data.label {
        node.set_label(label.as_str());
    }
    if role == Role::Label {
        // Static text is named by its value: the Windows and AT-SPI adapters
        // read a `Label`'s name from `value` alone
        // (`accesskit_consumer::Node::label_comes_from_value`), so a `Text`
        // published with only a label reached UI Automation and AT-SPI with an
        // empty name (the AppKit adapter falls back to the label, which is why
        // VoiceOver read it). The label stays for queries by label. A value the
        // node also carries follows the label on its own line, the separator
        // the reference joins merged labels with.
        match (&data.label, &data.value) {
            (Some(label), Some(value)) => node.set_value(format!("{label}\n{value}")),
            (Some(text), None) | (None, Some(text)) => node.set_value(text.as_str()),
            (None, None) => {}
        }
    } else if let Some(value) = &data.value {
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

    // AccessKit's set-position pair, which is what a screen reader reads out
    // as "item 12 of 100". `position_in_set` is ONE-based and documented as
    // never exceeding the container's `size_of_set`, so the zero-based
    // framework index converts here and a negative one (a caller's own
    // arithmetic underflowing an offset) is dropped rather than published as
    // a nonsensical position.
    //
    // Divergence from the reference, recorded in flui-semantics'
    // `## Mapping decisions`: Flutter carries `indexInParent` and
    // `scrollChildCount` as separate fields and leaves each platform bridge to
    // reconcile them into whatever that platform's set-position concept is.
    // AccessKit has the concept directly, so the pair is emitted here.
    if let Some(index) = data.index_in_parent
        && index >= 0
    {
        node.set_position_in_set(index as usize + 1);
    }
    if let Some(count) = data.scroll_child_count
        && count >= 0
    {
        node.set_size_of_set(count as usize);
    }

    apply_state(&mut node, data.flags);
    apply_actions(&mut node, data.actions);

    node.set_children(
        data.children
            .iter()
            .map(|child| NodeId(child.as_u64()))
            .collect::<Vec<_>>(),
    );

    node
}

/// A node as the adapter is handed it: [`to_node`], except that the root of a
/// window's tree is a [`Role::Window`] when nothing gave it a role — no
/// explicit role, and no flag or label that resolves one.
///
/// AccessKit's filter keeps a `GenericContainer` only while it holds the
/// focus (`accesskit_consumer::common_filter`), and the window's own UI
/// Automation element is the root node. With a container root, the first Tab
/// that moved the focus off the root took the root out of the tree a screen
/// reader walks, and navigation from the window stopped at its first child —
/// found through `cargo xtask device windows-input`. The AppKit adapter reads
/// a `Window` root the same way (`accesskit_macos`, `NodeWrapper::title`).
#[must_use]
pub(crate) fn to_published_node(data: &SemanticsNodeData, is_root: bool) -> Node {
    let mut node = to_node(data);
    // Only a root no role reached: an explicit role AccessKit can only
    // express as a container (`DragHandle`, `HotKey`) keeps that container.
    if is_root && data.role == SemanticsRole::None && node.role() == Role::GenericContainer {
        node.set_role(Role::Window);
    }
    node
}

/// The node claiming [`SemanticsFlag::IsFocused`], if exactly one does.
///
/// Two nodes claiming focus is a malformed tree, and guessing between them
/// would make the published focus depend on arena order. Reporting `None` sends
/// focus to the root, which is at least a node the adapter has definitely seen.
fn focused_node(tree: &crate::tree::SemanticsTree) -> Option<flui_foundation::SemanticsId> {
    let mut claiming = tree
        .iter()
        .filter(|(_, node)| node.config().is_focused())
        .map(|(id, _)| id);

    let first = claiming.next()?;
    if claiming.next().is_some() {
        tracing::warn!("semantics tree has more than one focused node; publishing the root");
        return None;
    }
    Some(first)
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
/// # Focus
///
/// An explicit `focus` wins. Otherwise focus is derived from the node carrying
/// [`SemanticsFlag::IsFocused`], because that is where the framework records it
/// — a caller should not have to re-derive what the tree already states, and a
/// caller that forgets would silently publish the root as focused. Focus falls
/// back to the root only when no node claims it, or when the named node is not
/// in the published set: AccessKit requires a valid target, and pointing at a
/// node the adapter has never seen is worse than pointing at the root.
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
        .filter_map(|(_, node)| {
            // `node_data_of` skips an unaddressable node (returns `None`),
            // resolves the children into the same stable space with the
            // identical skip rule, and works from the reference already in
            // hand — no second arena lookup per node on the publish path.
            let data = tree.node_data_of(node)?;
            let node_id = NodeId(data.id?.as_u64());
            Some((node_id, to_published_node(&data, node_id == root_node_id)))
        })
        .collect();

    let focus = focus
        .or_else(|| focused_node(tree))
        .and_then(stable_id)
        .filter(|wanted| nodes.iter().any(|(id, _)| id == wanted))
        .unwrap_or(root_node_id);

    Some(TreeUpdate {
        nodes,
        tree: Some(TreeInfo::new(root_node_id)),
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
        to_node(data)
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

    /// Every payload kind with an FLUI argument shape crosses the seam with
    /// its data intact; every kind without one returns `None` (the caller
    /// routes argument-free). A payload silently mistranslated here turns a
    /// screen-reader edit into the wrong edit, which is worse than a drop.
    #[test]
    fn action_payloads_translate_or_decline_honestly() {
        let target = NodeId(7);

        assert_eq!(
            semantics_action_args_for(&accesskit::ActionData::Value("hello".into()), target),
            Some(crate::ActionArgs::SetText {
                text: "hello".to_string()
            }),
        );
        assert_eq!(
            semantics_action_args_for(&accesskit::ActionData::CustomAction(42), target),
            Some(crate::ActionArgs::CustomAction { action_id: 42 }),
        );
        assert_eq!(
            semantics_action_args_for(
                &accesskit::ActionData::SetScrollOffset(accesskit::Point { x: 3.0, y: -4.5 }),
                target,
            ),
            Some(crate::ActionArgs::ScrollToOffset { x: 3.0, y: -4.5 }),
        );
        assert_eq!(
            semantics_action_args_for(
                &accesskit::ActionData::SetTextSelection(accesskit::TextSelection {
                    anchor: accesskit::TextPosition {
                        node: target,
                        character_index: 2,
                    },
                    focus: accesskit::TextPosition {
                        node: target,
                        character_index: 9,
                    },
                }),
                target,
            ),
            Some(crate::ActionArgs::SetSelection { base: 2, extent: 9 }),
        );

        // Kinds FLUI has no argument shape for decline rather than guess.
        assert_eq!(
            semantics_action_args_for(&accesskit::ActionData::NumericValue(0.5), target),
            None,
        );
    }

    /// A selection whose positions live on a different node than the request
    /// targets cannot be expressed as FLUI's within-one-node offsets —
    /// mapping it anyway would apply another run's indices to this node's
    /// text.
    #[test]
    fn a_cross_node_text_selection_declines() {
        let target = NodeId(7);
        let selection = accesskit::ActionData::SetTextSelection(accesskit::TextSelection {
            anchor: accesskit::TextPosition {
                node: NodeId(8),
                character_index: 0,
            },
            focus: accesskit::TextPosition {
                node: target,
                character_index: 3,
            },
        });
        assert_eq!(semantics_action_args_for(&selection, target), None);
    }

    /// The two action tables' agreement, checked as a composition: every
    /// AccessKit action in [`semantics_action_for`]'s domain, when granted
    /// to a node as the FLUI action it maps to, must be advertised outbound
    /// again by [`apply_actions`]. A drifted pair makes a control assistive
    /// technology can see and press but that does nothing. (The reverse
    /// drift — advertised outbound but not yet routable — is a review
    /// obligation stated on both tables' docs; nothing here enumerates
    /// AccessKit's action space to catch it mechanically.)
    #[test]
    fn every_inbound_routable_action_is_advertised_outbound_again() {
        const INBOUND_DOMAIN: &[accesskit::Action] = &[
            accesskit::Action::Click,
            accesskit::Action::ShowContextMenu,
            accesskit::Action::ScrollLeft,
            accesskit::Action::ScrollRight,
            accesskit::Action::ScrollUp,
            accesskit::Action::ScrollDown,
            accesskit::Action::Increment,
            accesskit::Action::Decrement,
            accesskit::Action::ScrollIntoView,
            accesskit::Action::SetTextSelection,
            accesskit::Action::SetValue,
            accesskit::Action::SetScrollOffset,
            accesskit::Action::Focus,
            accesskit::Action::Blur,
            accesskit::Action::CustomAction,
        ];
        for &inbound in INBOUND_DOMAIN {
            let routed = semantics_action_for(inbound)
                .expect("every action in the declared inbound domain is routable");
            let data = SemanticsNodeData {
                actions: routed as u64,
                ..Default::default()
            };
            assert!(
                translate(&data).supports_action(inbound),
                "{inbound:?} routes inbound to {routed:?}, whose outbound translation no \
                 longer advertises {inbound:?} — the two tables have drifted",
            );
        }
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

    /// The precedence leg, and the one that a single-flag test cannot reach.
    ///
    /// An annotated ancestor absorbs its descendants' flags by union, so a radio
    /// inside a button-like container arrives carrying the container's
    /// `IsButton` beside its own checkable flags. The test above passes whether
    /// or not the cascade tests `IsButton` first, because it never sets that
    /// flag; this one fails the moment the checkable arm moves back below it.
    #[test]
    fn a_checkable_beside_is_button_still_resolves_to_the_checkable() {
        let button_shaped_radio = SemanticsNodeData {
            flags: flags(&[
                SemanticsFlag::IsButton,
                SemanticsFlag::HasCheckedState,
                SemanticsFlag::IsInMutuallyExclusiveGroup,
            ]),
            ..Default::default()
        };
        assert_eq!(translate(&button_shaped_radio).role(), Role::RadioButton);

        let button_shaped_checkbox = SemanticsNodeData {
            flags: flags(&[SemanticsFlag::IsButton, SemanticsFlag::HasCheckedState]),
            ..Default::default()
        };
        assert_eq!(translate(&button_shaped_checkbox).role(), Role::CheckBox);

        let button_shaped_switch = SemanticsNodeData {
            flags: flags(&[SemanticsFlag::IsButton, SemanticsFlag::HasToggledState]),
            ..Default::default()
        };
        assert_eq!(translate(&button_shaped_switch).role(), Role::Switch);

        // The premise, so a reader can see the arm is not vacuous: the flag that
        // loses precedence really is present on each node above.
        let button_only = SemanticsNodeData {
            flags: flags(&[SemanticsFlag::IsButton]),
            ..Default::default()
        };
        assert_eq!(translate(&button_only).role(), Role::Button);
    }

    /// A node with a label and no role-bearing flag is static text, not a
    /// generic container: AccessKit's consumer filter drops
    /// `GenericContainer` from what an assistive technology sees, so the
    /// old resolution made every plain `Text` invisible to VoiceOver
    /// (`cargo xtask device macos-a11y`, 2026-09-22). An unlabelled, flagless
    /// node stays a container, and a label does not override a real flag.
    #[test]
    fn a_labelled_flagless_node_is_static_text() {
        let text = SemanticsNodeData {
            label: Some("You have pushed the button this many times:".into()),
            ..Default::default()
        };
        assert_eq!(translate(&text).role(), Role::Label);

        let empty_label = SemanticsNodeData {
            label: Some("".into()),
            ..Default::default()
        };
        assert_eq!(translate(&empty_label).role(), Role::GenericContainer);
        assert_eq!(
            translate(&SemanticsNodeData::default()).role(),
            Role::GenericContainer
        );

        let labelled_button = SemanticsNodeData {
            flags: flags(&[SemanticsFlag::IsButton]),
            label: Some("Increment".into()),
            ..Default::default()
        };
        assert_eq!(translate(&labelled_button).role(), Role::Button);
    }

    /// The name an assistive technology reads for each node, as the AccessKit
    /// adapters derive it: a `Label`'s from its value, anything else's from its
    /// label. Read through `accesskit_consumer`, the layer every adapter sits on.
    fn adapter_names(nodes: &[(u64, Node)]) -> Vec<(Role, Option<String>)> {
        let mut root = Node::new(Role::Window);
        root.set_children(nodes.iter().map(|(id, _)| NodeId(*id)).collect::<Vec<_>>());
        let mut all = vec![(NodeId(1), root)];
        all.extend(nodes.iter().map(|(id, node)| (NodeId(*id), node.clone())));
        let tree = accesskit_consumer::Tree::new(
            TreeUpdate {
                nodes: all,
                tree: Some(TreeInfo::new(NodeId(1))),
                tree_id: TreeId::ROOT,
                focus: NodeId(1),
            },
            true,
        );
        tree.state()
            .root()
            .children()
            .map(|node| {
                let name = if node.label_comes_from_value() {
                    node.value()
                } else {
                    node.label()
                };
                (node.role(), name)
            })
            .collect()
    }

    /// A plain `Text` is named by its text on every adapter. Found on the first
    /// live Windows run (`cargo xtask device windows-a11y`): UI Automation
    /// reported the counter's two texts with empty names beside a correctly
    /// named button, because the text went out as a label and an adapter reads
    /// a `Label`'s name from its value.
    #[test]
    fn static_text_is_named_by_its_text() {
        let text = SemanticsNodeData {
            label: Some("You have pushed the button this many times:".into()),
            ..Default::default()
        };
        let labelled_value = SemanticsNodeData {
            label: Some("Volume".into()),
            value: Some("40%".into()),
            ..Default::default()
        };
        let button = SemanticsNodeData {
            flags: flags(&[SemanticsFlag::IsButton]),
            label: Some("Increment".into()),
            ..Default::default()
        };

        let names = adapter_names(&[
            (2, translate(&text)),
            (3, translate(&labelled_value)),
            (4, translate(&button)),
        ]);

        assert_eq!(
            names,
            vec![
                (
                    Role::Label,
                    Some("You have pushed the button this many times:".into())
                ),
                (Role::Label, Some("Volume\n40%".into())),
                (Role::Button, Some("Increment".into())),
            ]
        );
    }

    /// `IsButton` outranks `IsLink` and `IsTextField`, and keeps doing so.
    ///
    /// The two arms the reorder deliberately left below `IsButton`. This is a
    /// recorded divergence rather than an oversight: the reference publishes both
    /// flags on one node and lets the platform read what it wants
    /// (`test/material/dropdown_menu_test.dart`'s `'ensure exclude semantics for
    /// trailing button'`, `test/widgets/semantics_merge_test.dart`'s `'LinkUri
    /// from child is passed up to the parent when merging nodes'`), where `Role`
    /// is single-valued and has to choose. The choice is recorded in
    /// `crates/flui-semantics/ARCHITECTURE.md` mapping decision 2, and this test
    /// is what keeps a later reorder from changing the answer quietly.
    #[test]
    fn is_link_and_is_text_field_lose_to_is_button_as_they_always_have() {
        let button_shaped_link = SemanticsNodeData {
            flags: flags(&[SemanticsFlag::IsButton, SemanticsFlag::IsLink]),
            ..Default::default()
        };
        // The losing flag is in the fixture rather than assumed: without it this
        // is the `IsButton`-only case, which pins nothing.
        assert!(has_flag(button_shaped_link.flags, SemanticsFlag::IsLink));
        assert_eq!(translate(&button_shaped_link).role(), Role::Button);

        let button_shaped_text_field = SemanticsNodeData {
            flags: flags(&[SemanticsFlag::IsButton, SemanticsFlag::IsTextField]),
            ..Default::default()
        };
        assert!(has_flag(
            button_shaped_text_field.flags,
            SemanticsFlag::IsTextField
        ));
        assert_eq!(translate(&button_shaped_text_field).role(), Role::Button);

        // The premise: alone, each of the two flags does win, so the nodes above
        // are resolving to `Button` because of `IsButton` and not because the
        // losing flag went unset.
        let link_only = SemanticsNodeData {
            flags: flags(&[SemanticsFlag::IsLink]),
            ..Default::default()
        };
        assert_eq!(translate(&link_only).role(), Role::Link);

        let text_field_only = SemanticsNodeData {
            flags: flags(&[SemanticsFlag::IsTextField]),
            ..Default::default()
        };
        assert_eq!(translate(&text_field_only).role(), Role::TextInput);
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

    /// The window's root stays in the tree a screen reader walks after the
    /// focus moves into it. Found on a live window: the root went out as a
    /// `GenericContainer`, which AccessKit keeps only while it is focused, so
    /// the first Tab to a button took the root out and UI Automation stopped
    /// walking the window after its first child.
    ///
    /// Red-check: publish the root through `to_node` instead of
    /// `to_published_node` — the root is filtered out once the button holds
    /// the focus.
    #[test]
    fn the_root_stays_visible_when_the_focus_moves_into_the_tree() {
        let mut tree = SemanticsTree::new();
        let mut children = Vec::new();
        for (index, label) in [(40, "prompt"), (41, "0"), (42, "Increment")] {
            let mut node = SemanticsNode::new().with_source_render_id(render_id(index));
            node.config_mut().set_label(label);
            if label == "Increment" {
                node.config_mut().set_button(true);
            }
            children.push(tree.insert(node));
        }
        let mut root_node = SemanticsNode::new().with_source_render_id(render_id(2));
        for &child in &children {
            root_node.add_child(child);
        }
        let root = tree.insert(root_node);
        tree.set_root(Some(root));

        let update = tree_to_update(&tree, Some(children[2])).expect("rooted");
        let consumer = accesskit_consumer::Tree::new(update, true);
        let state = consumer.state();
        let root = state.root();

        assert_eq!(root.role(), Role::Window);
        assert_eq!(
            accesskit_consumer::common_filter(&root),
            accesskit_consumer::FilterResult::Include,
            "the root is part of the walked tree with the focus on the button"
        );
        let reachable: Vec<_> = root
            .filtered_children(accesskit_consumer::common_filter)
            .map(|node| node.role())
            .collect();
        assert_eq!(reachable, vec![Role::Label, Role::Label, Role::Button]);
    }

    /// A root with an explicit role AccessKit can only express as a container
    /// keeps that container: only a root no role reached becomes a `Window`.
    ///
    /// Red-check: gate the promotion on the translated role alone — the
    /// drag-handle root is published as a window.
    #[test]
    fn an_explicit_container_role_on_the_root_is_not_promoted_to_a_window() {
        let mut tree = SemanticsTree::new();
        let mut root_node = SemanticsNode::new().with_source_render_id(render_id(2));
        root_node.config_mut().set_role(SemanticsRole::DragHandle);
        let root = tree.insert(root_node);
        tree.set_root(Some(root));

        let update = tree_to_update(&tree, None).expect("rooted");
        let (_, published) = &update.nodes[0];
        assert_eq!(published.role(), Role::GenericContainer);
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

    /// **The focus contract.** A focused non-root control must be published as
    /// the focus target. Deriving it from the tree is what stops every caller
    /// that passes `None` from silently announcing the root as focused.
    #[test]
    fn focus_is_derived_from_the_focused_node_when_the_caller_names_none() {
        let mut tree = SemanticsTree::new();
        let child_render = render_id(31);
        let mut child_node = SemanticsNode::new().with_source_render_id(child_render);
        child_node.config_mut().set_focused(true);
        let child = tree.insert(child_node);

        let mut root_node = SemanticsNode::new().with_source_render_id(render_id(30));
        root_node.add_child(child);
        let root = tree.insert(root_node);
        tree.set_root(Some(root));

        let update = tree_to_update(&tree, None).expect("rooted");

        assert_eq!(
            update.focus,
            NodeId(AccessibilityNodeId::from(child_render).as_u64()),
            "the focused control, not the root"
        );
        assert_ne!(
            update.focus,
            update.tree.as_ref().expect("tree").root,
            "the fixture is only meaningful while the two differ"
        );
    }

    /// An explicit target overrides the flag, so a caller can publish a focus
    /// the tree does not yet record.
    #[test]
    fn an_explicit_focus_overrides_the_focused_flag() {
        let mut tree = SemanticsTree::new();
        let mut flagged = SemanticsNode::new().with_source_render_id(render_id(41));
        flagged.config_mut().set_focused(true);
        let flagged_id = tree.insert(flagged);

        let other_render = render_id(42);
        let other = tree.insert(SemanticsNode::new().with_source_render_id(other_render));

        let mut root_node = SemanticsNode::new().with_source_render_id(render_id(40));
        root_node.add_child(flagged_id);
        root_node.add_child(other);
        let root = tree.insert(root_node);
        tree.set_root(Some(root));

        let update = tree_to_update(&tree, Some(other)).expect("rooted");
        assert_eq!(
            update.focus,
            NodeId(AccessibilityNodeId::from(other_render).as_u64())
        );
    }

    /// Two nodes claiming focus is malformed. Picking one would make the
    /// published focus depend on arena order, so it falls back to the root.
    #[test]
    fn two_focused_nodes_fall_back_to_the_root() {
        let mut tree = SemanticsTree::new();
        let mut make_focused = |index: u32| {
            let mut node = SemanticsNode::new().with_source_render_id(render_id(index));
            node.config_mut().set_focused(true);
            tree.insert(node)
        };
        let first = make_focused(51);
        let second = make_focused(52);

        let mut root_node = SemanticsNode::new().with_source_render_id(render_id(50));
        root_node.add_child(first);
        root_node.add_child(second);
        let root = tree.insert(root_node);
        tree.set_root(Some(root));

        let update = tree_to_update(&tree, None).expect("rooted");
        assert_eq!(update.focus, update.tree.as_ref().expect("tree").root);
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
