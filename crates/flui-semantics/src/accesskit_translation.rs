//! Translation from FLUI's semantics tree into the AccessKit tree model.
//!
//! AccessKit is the shape both OS accessibility adapters and query-by-role UI
//! testing consume, so this translation is deliberately the *only* place the two
//! diverge from each other: whatever a screen reader is told, a test can assert.
//!
//! # Role is carried in two places
//!
//! FLUI encodes role at two granularities:
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
/// [`Role::Label`] — static text — and a node with neither becomes
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
    // real widget trees do combine `IsButton` with `IsTextField` (a dropdown's
    // trailing button) and with `IsLink` (a merged link child). FLUI's `Role` is
    // single-valued rather than a flag set, so `IsButton`
    // outranking them is deliberate rather than an absence of
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
        // `SemanticsRole::None`, which declares no role. SemanticsRole is
        // non_exhaustive, so this arm would also take a role added later; the
        // pin is `roles_and_checkbox_states_translate_to_accesskit`, not this match.
        _ => return None,
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
///
/// A node with an expanded state and a tap handler also advertises the one
/// transition its state allows — `Expand` while collapsed, `Collapse` while
/// expanded — because [`semantics_action_for`] routes both to its tap handler,
/// which is how FLUI toggles an expandable node. AccessKit does not count a
/// node with an expanded state as invocable (`accesskit_consumer` 0.39,
/// `Node::is_invocable`), so its Windows adapter offers UI Automation's
/// `ExpandCollapse` pattern for it and no `Invoke`; without these an agent or a
/// screen reader could neither invoke nor expand such a node.
fn apply_actions(node: &mut Node, actions: u64, flags: u64) {
    if has_action(actions, SemanticsAction::Tap) {
        node.add_action(accesskit::Action::Click);
        if has_flag(flags, SemanticsFlag::HasExpandedState) {
            node.add_action(if has_flag(flags, SemanticsFlag::IsExpanded) {
                accesskit::Action::Collapse
            } else {
                accesskit::Action::Expand
            });
        }
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
/// Three deliberate asymmetries against the outbound table:
///
/// - `Focus` maps to [`SemanticsAction::Focus`] only. Outbound, a node
///   registering only the legacy `DidGainAccessibilityFocus` *notification*
///   also advertises `Focus`, but the platform's focus request is dispatched
///   as the `focus` action itself — the legacy handler is a notification
///   hook, not the action's implementation.
/// - `Blur` maps to `DidLoseAccessibilityFocus`, which IS the notification,
///   because that is the only vocabulary FLUI has for it.
/// - `Expand` and `Collapse` have no FLUI action. They reach the node's tap
///   handler ([`SemanticsAction::Tap`]), which is how FLUI toggles an
///   expandable node, and `apply_actions` advertises only the one its expanded
///   state allows. The adapter that emits them refuses a transition to the
///   state the node already has (accesskit_windows 0.35.0, `node.rs`
///   `ExpandCollapse` provider); no other shipped adapter emits them. That
///   check reads the adapter's copy of the tree, which lags until the next
///   published update, so two `Expand`s before the next frame both pass it and
///   both toggle; the routed `Tap` carries no direction for FLUI to check.
///   Discrete `Expand`/`Collapse` actions would close this
///   (`crates/flui-semantics/ARCHITECTURE.md`, mapping decision 5).
///
/// `SetValue` lands on [`SemanticsAction::SetText`] whatever its payload: a
/// numeric value (UI Automation's `RangeValue.SetValue`) arrives without its
/// number, because [`semantics_action_args_for`] has no argument shape for it.
///
/// The match names every AccessKit action, with no wildcard arm:
/// `accesskit::Action` is not `#[non_exhaustive]`, so an upstream release that
/// adds one stops this compiling until someone decides whether FLUI routes it
/// (ADR-0089 §3).
#[must_use]
pub fn semantics_action_for(action: accesskit::Action) -> Option<SemanticsAction> {
    match action {
        // Expand and collapse toggle through the tap handler (see above).
        accesskit::Action::Click | accesskit::Action::Expand | accesskit::Action::Collapse => {
            Some(SemanticsAction::Tap)
        }
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
        accesskit::Action::HideTooltip
        | accesskit::Action::ShowTooltip
        | accesskit::Action::ReplaceSelectedText
        | accesskit::Action::ScrollToPoint
        | accesskit::Action::SetSequentialFocusNavigationStartingPoint => None,
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
        accesskit::ActionData::NumericValue(_)
        | accesskit::ActionData::ScrollUnit(_)
        | accesskit::ActionData::ScrollHint(_)
        | accesskit::ActionData::ScrollToPoint(_) => None,
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
        x0: data.rect.left(),
        y0: data.rect.top(),
        x1: data.rect.right(),
        y1: data.rect.bottom(),
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
    // Recorded in flui-semantics' `## Mapping decisions`: AccessKit has the
    // set-position concept directly, so the index/count pair is emitted here
    // rather than left for each platform bridge to reconcile.
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
    apply_actions(&mut node, data.actions, data.flags);

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
            accesskit::Action::Expand,
            accesskit::Action::Collapse,
        ];
        for &inbound in INBOUND_DOMAIN {
            let routed = semantics_action_for(inbound)
                .expect("every action in the declared inbound domain is routable");
            // Expand and collapse are advertised only by a node with an
            // expanded state, and only the transition that state allows.
            let state = match inbound {
                accesskit::Action::Expand => flags(&[SemanticsFlag::HasExpandedState]),
                accesskit::Action::Collapse => {
                    flags(&[SemanticsFlag::HasExpandedState, SemanticsFlag::IsExpanded])
                }
                _ => 0,
            };
            let data = SemanticsNodeData {
                actions: routed.value(),
                flags: state,
                ..Default::default()
            };
            assert!(
                translate(&data).supports_action(inbound),
                "{inbound:?} routes inbound to {routed:?}, whose outbound translation no \
                 longer advertises {inbound:?} — the two tables have drifted",
            );
        }
    }

    /// The role mapping's only pin. `SemanticsRole` is `#[non_exhaustive]` and
    /// lives in flui-protocol, so `explicit_role` ends in a wildcard: deleting
    /// any arm there sends that role to `None` (the flags decide) and nothing
    /// but this test notices.
    #[test]
    fn roles_and_checkbox_states_translate_to_accesskit() {
        let mut generic = Vec::new();
        for &role in SemanticsRole::ALL {
            let mapped = explicit_role(role);
            if role == SemanticsRole::None {
                assert_eq!(mapped, None, "`none` declares no role; the flags decide");
                continue;
            }
            let mapped = mapped.unwrap_or_else(|| panic!("{role} maps to no AccessKit role"));
            if mapped == Role::GenericContainer {
                generic.push(role);
            }
        }
        // The documented no-counterpart roles, and only those.
        assert_eq!(
            generic,
            [SemanticsRole::DragHandle, SemanticsRole::HotKey],
            "only the roles AccessKit has no counterpart for may be generic",
        );

        // Checkbox states: unchecked, checked and mixed stay three distinct values.
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

    /// **The identity contract.** AccessKit ids must be the stable
    /// `AccessibilityNodeId` (a packed generational `RenderId`), never the
    /// arena position. The arena positions here are 1 and 2; the render
    /// identities are deliberately unrelated numbers, so a translation that
    /// leaked `SemanticsId` would produce visibly different ids.
    #[test]
    fn node_ids_are_the_stable_render_identity_not_the_arena_position_or_a_recycled_slot() {
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

        // A recycled slot (same index, next generation) is a distinct identity.
        let recycled = flui_foundation::RenderId::new_gen(
            7,
            core::num::NonZeroU32::new(4).expect("fixture generation is non-zero"),
        );
        let first = flui_foundation::RenderId::new_gen(
            7,
            core::num::NonZeroU32::new(3).expect("fixture generation is non-zero"),
        );
        assert_ne!(
            AccessibilityNodeId::from(first),
            AccessibilityNodeId::from(recycled),
            "a recycled arena slot must not reuse the previous occupant's accessibility id"
        );

        // The owner-level entry point (platform bridge and harness alike) routes
        // through the same translation and focuses the root.
        let mut owner = crate::owner::SemanticsOwner::new_without_callback();
        let owner_root = owner
            .tree_mut()
            .insert(SemanticsNode::new().with_source_render_id(root_render));
        owner.tree_mut().set_root(Some(owner_root));
        let owner_update = owner
            .to_accesskit_tree_update(None)
            .expect("a rooted tree yields an update");
        assert_eq!(
            owner_update.tree.as_ref().expect("tree").root,
            expected_root
        );
        assert_eq!(owner_update.focus, expected_root);
        assert_eq!(owner_update.nodes.len(), 1);
    }
}
