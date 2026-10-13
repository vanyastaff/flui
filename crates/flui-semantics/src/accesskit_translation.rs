//! Translation from FLUI's semantics tree into the AccessKit tree model.
//!
//! AccessKit is the shape both OS accessibility adapters and query-by-role UI
//! testing consume, so this translation is deliberately the *only* place the two
//! diverge from each other: whatever a screen reader is told, a test can assert.
//!
//! # One role, typed states
//!
//! A node has one [`SemanticsRole`] and typed [`States`]
//! (ADR-0206). [`resolve_role`] maps the role; a node that declares none is
//! static text when it is named and a plain container otherwise. Conflicting
//! roles never reach here: merging settled them
//! ([`more_specific_role`](crate::configuration::more_specific_role)).
//!
//! # Full updates, not diffs
//!
//! `TreeUpdate` documents that an update "should only include nodes that are new
//! or changed". FLUI's assembly is a classic full rebuild (flui-semantics ARCHITECTURE.md, semantics assembly), so every
//! pass yields every node and this emits all of them. Platform adapters suppress
//! extraneous events, so that is correct but not free. Incremental diffing is a
//! later optimisation and needs its own oracle; it is not smuggled in here.

use accesskit::{Affine, Node, NodeId, Rect, Role, TextDirection, TreeId, TreeInfo, TreeUpdate};

use crate::action::SemanticsAction;
use crate::attributes::SemanticsAttributes;
use crate::identity::AccessibilityNodeId;
use crate::node::SemanticsNode;
use crate::relation::{SemanticsAnchor, SemanticsRelation};
use crate::role::{Assertiveness, SemanticsRole};
use crate::states::{States, Toggled};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

/// One addressable node as publication reads it: its attributes in place,
/// and what only the tree knows — its stable identity, effective actions,
/// bounds and the stable identities of its addressable children.
#[derive(Debug)]
pub(crate) struct PublishView<'a> {
    pub(crate) id: AccessibilityNodeId,
    pub(crate) attrs: &'a SemanticsAttributes,
    /// Effective action bits (`blocks_user_actions` already applied).
    pub(crate) actions: u64,
    pub(crate) rect: flui_foundation::geometry::Rect<f64>,
    pub(crate) children: SmallVec<[AccessibilityNodeId; 4]>,
}

/// Whether `bits` carries `action`.
#[inline]
fn has_action(bits: u64, action: SemanticsAction) -> bool {
    bits & (action as u64) != 0
}

/// The AccessKit role for one node.
///
/// The node's role when it declares one. Otherwise a node that carries a label
/// (a plain `Text`, a labelled `Semantics` wrapper) is a [`Role::Label`] —
/// static text — and a node with neither becomes [`Role::GenericContainer`],
/// present in the tree for structure but making no claim about what it is.
///
/// The distinction is load-bearing at the OS boundary: AccessKit's consumer
/// filter (`accesskit_consumer::common_filter`) drops `GenericContainer` nodes
/// from what an assistive technology sees, so a labelled text that resolved to
/// it was invisible to VoiceOver — observed on 2026-09-22 through
/// `cargo xtask device macos-a11y`, where the counter's two `Text`s were absent from the
/// `AXUIElement` tree while its button was present.
#[must_use]
pub(crate) fn resolve_role(data: &PublishView<'_>) -> Role {
    if let Some(role) = explicit_role(data.attrs.role) {
        return role;
    }
    if data
        .attrs
        .label
        .as_ref()
        .is_some_and(|label| !label.is_empty())
    {
        Role::Label
    } else {
        Role::GenericContainer
    }
}

/// The AccessKit role for a [`SemanticsRole`], or `None` when the node
/// declares no role.
fn explicit_role(role: SemanticsRole) -> Option<Role> {
    Some(match role {
        SemanticsRole::Button => Role::Button,
        SemanticsRole::CheckBox => Role::CheckBox,
        SemanticsRole::RadioButton => Role::RadioButton,
        SemanticsRole::Switch => Role::Switch,
        SemanticsRole::Link => Role::Link,
        SemanticsRole::Slider => Role::Slider,
        SemanticsRole::Image => Role::Image,
        // A heading, not AccessKit's `Header`: that is HTML's `<header>`
        // landmark, which `accesskit_windows` 0.35 reports as a `Group` with
        // the `banner` landmark type, so a title published as one never
        // reached heading navigation.
        SemanticsRole::Heading => Role::Heading,
        SemanticsRole::TextInput => Role::TextInput,
        SemanticsRole::MultilineTextInput => Role::MultilineTextInput,
        SemanticsRole::PasswordInput => Role::PasswordInput,
        SemanticsRole::ScrollView => Role::ScrollView,
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

/// How a node's changes are announced: its declared politeness, else what its
/// role implies (`aria-live` is `assertive` for an alert and `polite` for a
/// status), else not at all.
fn live_politeness(live: Option<Assertiveness>, role: SemanticsRole) -> Option<accesskit::Live> {
    match (live, role) {
        (Some(Assertiveness::Assertive), _) | (None, SemanticsRole::Alert) => {
            Some(accesskit::Live::Assertive)
        }
        (Some(Assertiveness::Polite), _) | (None, SemanticsRole::Status) => {
            Some(accesskit::Live::Polite)
        }
        (None, _) => None,
    }
}

/// Translate one node's states onto the AccessKit node.
fn apply_state(node: &mut Node, attrs: &SemanticsAttributes) {
    // Exhaustive: a new state does not compile until it is published here
    // or recorded as framework-only.
    let States {
        toggled,
        selected,
        expanded,
        enabled,
        read_only,
        focusable: _,
        focused: _,
        hidden,
        modal,
        required,
        invalid,
        scopes_route: _,
        names_route: _,
    } = attrs.states;

    if let Some(toggled) = toggled {
        node.set_toggled(match toggled {
            Toggled::Off => accesskit::Toggled::False,
            Toggled::On => accesskit::Toggled::True,
            Toggled::Mixed => accesskit::Toggled::Mixed,
        });
    }
    // A declared `false` is published too: AccessKit tells an unselected
    // item from one selection does not apply to.
    if let Some(selected) = selected {
        node.set_selected(selected);
    }
    if let Some(expanded) = expanded {
        node.set_expanded(expanded);
    }
    // Only a declared `false` disables: a node that never declared it has no
    // such concept.
    if enabled == Some(false) {
        node.set_disabled();
    }
    if read_only == Some(true) {
        node.set_read_only();
    }
    if hidden == Some(true) {
        node.set_hidden();
    }
    if let Some(live) = live_politeness(attrs.live, attrs.role) {
        node.set_live(live);
    }
    if modal == Some(true) {
        node.set_modal();
    }
    if required == Some(true) {
        node.set_required();
    }
    if invalid == Some(true) {
        node.set_invalid(accesskit::Invalid::True);
    }
}

/// Whether some platform value write reaches a handler on this node, which is
/// when it advertises `SetValue` and, for a range, when it is not read-only.
///
/// The handler follows the payload, not the node's shape:
/// [`semantics_action_request_for`] sends a string (`ActionData::Value`) to
/// `SetText` and a number (`ActionData::NumericValue`) to `SetNumericValue`.
/// `accesskit_windows` 0.35 offers the `Value` pattern on a node with a text
/// value and the `RangeValue` pattern on one with a numeric value,
/// independently (`node.rs`, `is_value_pattern_supported` and
/// `is_range_value_pattern_supported`), and their `SetValue` methods send a
/// string and a number respectively. `accesskit_macos` 0.27 sends an
/// `NSString` as a string and an `NSNumber` as a number from
/// `setAccessibilityValue:`; `accesskit_atspi_common` 0.21 sends AT-SPI's
/// `Value.CurrentValue` as a number. So a range is writable through its
/// numeric handler whatever text it also shows, and through its text handler
/// when it shows text. A node without a range is written as text only. A
/// static label's value is its name (`Node::label_comes_from_value`), not a
/// writable value.
///
/// AccessKit has one read-only flag, which `accesskit_windows` reports as
/// both patterns' `IsReadOnly`, so a range showing text with only one of the
/// two handlers is published writable through both. The owner offers a
/// `Value` string to a range without a text handler as a number
/// ([`SemanticsOwner::resolve_action`](crate::SemanticsOwner::resolve_action)),
/// so only a string that is not a number is refused there; a range with
/// only a text handler refuses every `RangeValue` write. Each refusal comes
/// after the adapter reported the write accepted; marking the node read-only
/// instead would hide the write that works.
fn value_writable(data: &PublishView<'_>, role: Role) -> bool {
    let has_text_value = data.attrs.value.is_some() && role != Role::Label;
    let text_reaches = data.attrs.numeric_range.is_none() || has_text_value;
    (text_reaches && has_action(data.actions, SemanticsAction::SetText))
        || (data.attrs.numeric_range.is_some()
            && has_action(data.actions, SemanticsAction::SetNumericValue))
}

/// Translate the supported actions onto the AccessKit node.
///
/// Several FLUI actions have no AccessKit counterpart and are intentionally not
/// emitted: the character- and word-wise cursor moves, and `Copy`/`Cut`/`Paste`
/// (AccessKit expects those to reach the app through the platform's own text
/// interface, not as tree actions). `Dismiss` likewise has no equivalent. They
/// are dropped rather than approximated, so nothing claims support it lacks.
///
/// A node with an expanded state advertises only the one transition its state
/// allows: `Expand` while collapsed, `Collapse` while expanded. It advertises
/// that transition when it registers the matching discrete action, or, when it
/// registers neither discrete action, when it has a tap handler; the owner
/// then routes the request to that handler
/// ([`tap_disclosure_transition`](crate::action::tap_disclosure_transition)).
/// AccessKit does not count a node with an expanded state as invocable
/// (`accesskit_consumer` 0.39, `Node::is_invocable`), so without this a
/// tap-only expandable node could be neither invoked nor expanded.
fn apply_actions(node: &mut Node, actions: u64, expanded: Option<bool>, value_writable: bool) {
    if has_action(actions, SemanticsAction::Tap) {
        node.add_action(accesskit::Action::Click);
    }
    if let Some(expanded) = expanded {
        let fallback = crate::action::tap_disclosure_transition(actions, Some(expanded));
        if !expanded
            && (has_action(actions, SemanticsAction::Expand)
                || fallback == Some(SemanticsAction::Expand))
        {
            node.add_action(accesskit::Action::Expand);
        }
        if expanded
            && (has_action(actions, SemanticsAction::Collapse)
                || fallback == Some(SemanticsAction::Collapse))
        {
            node.add_action(accesskit::Action::Collapse);
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
    // A string write reaches the text handler and a number the numeric one;
    // `value_writable` decides whether any write the platform can send here
    // has a handler (the owner refuses a number without a range to check it).
    if value_writable {
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
/// Two deliberate focus asymmetries against the outbound table:
///
/// - `Focus` maps to [`SemanticsAction::Focus`] only. Outbound, a node
///   registering only the legacy `DidGainAccessibilityFocus` *notification*
///   also advertises `Focus`, but the platform's focus request is dispatched
///   as the `focus` action itself — the legacy handler is a notification
///   hook, not the action's implementation.
/// - `Blur` maps to `DidLoseAccessibilityFocus`, which IS the notification,
///   because that is the only vocabulary FLUI has for it.
///
/// `Expand` and `Collapse` map to the discrete actions. A tap-only expandable
/// node still receives the transition its state allows through its tap
/// handler, which the owner resolves at dispatch. `SetValue` defaults to text
/// when only its action is known; [`semantics_action_request_for`] also
/// examines its payload to route numeric values distinctly.
///
/// The match names every AccessKit action, with no wildcard arm:
/// `accesskit::Action` is not `#[non_exhaustive]`, so an upstream release that
/// adds one stops this compiling until someone decides whether FLUI routes it
/// (ADR-0089 §3).
#[must_use]
pub fn semantics_action_for(action: accesskit::Action) -> Option<SemanticsAction> {
    match action {
        accesskit::Action::Click => Some(SemanticsAction::Tap),
        accesskit::Action::Expand => Some(SemanticsAction::Expand),
        accesskit::Action::Collapse => Some(SemanticsAction::Collapse),
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
/// argument shape for (`ScrollUnit`, `ScrollHint`,
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
        accesskit::ActionData::NumericValue(value) => {
            Some(crate::ActionArgs::SetNumericValue { value: *value })
        }
        accesskit::ActionData::ScrollUnit(_)
        | accesskit::ActionData::ScrollHint(_)
        | accesskit::ActionData::ScrollToPoint(_) => None,
    }
}

/// Translates an entire platform request without discarding numeric payloads.
///
/// Payload validity is checked by the owner against the current node before
/// invoking its handler. Unroutable platform actions return `None`.
#[must_use]
pub fn semantics_action_request_for(
    request: &accesskit::ActionRequest,
) -> Option<crate::SemanticsActionRequest> {
    let node_id = crate::AccessibilityNodeId::from_u64(request.target_node.0)?;
    let action = if request.action == accesskit::Action::SetValue
        && matches!(request.data, Some(accesskit::ActionData::NumericValue(_)))
    {
        SemanticsAction::SetNumericValue
    } else {
        semantics_action_for(request.action)?
    };
    let arguments = request
        .data
        .as_ref()
        .and_then(|data| semantics_action_args_for(data, request.target_node));
    Some(crate::SemanticsActionRequest {
        node_id,
        action,
        arguments,
    })
}

/// Translate one FLUI semantics node into an AccessKit node.
///
/// Children come from [`PublishView::children`], which carries the
/// stable [`AccessibilityNodeId`]s — the same
/// space AccessKit ids are published in, so the mapping is a transparent
/// repack. (An earlier payload shape held arena positions there, and this
/// function had to refuse them; the type now makes that leak
/// unrepresentable.)
#[must_use]
pub(crate) fn to_node(data: &PublishView<'_>) -> Node {
    // Exhaustive: an attribute added to `SemanticsAttributes` does not compile
    // until it is published here or its reason for not being published is
    // written down.
    let SemanticsAttributes {
        // Published below or by the helpers this function calls.
        role: _,
        states: _,
        live: _,
        label: _,
        value: _,
        numeric_range: _,
        hint: _,
        tooltip: _,
        placeholder: _,
        heading_level: _,
        text_direction: _,
        scroll_position: _,
        scroll_axis: _,
        scroll_extent_max: _,
        scroll_extent_min: _,
        index_in_parent: _,
        scroll_child_count: _,
        // Resolved against the anchor index by `to_published_node`.
        anchors: _,
        relations: _,
        // Framework-side: merge bookkeeping and traversal order, never
        // published as properties.
        tags: _,
        sort_key: _,
        hint_overrides: _,
        scroll_index: _,
        // Not published yet: AccessKit has no adjacent-value or platform-view
        // property, and custom-action metadata and text lengths join with the
        // works that route them (spec W6–W9).
        increased_value: _,
        decreased_value: _,
        platform_view_id: _,
        custom_actions: _,
        max_value_length: _,
        current_value_length: _,
    } = data.attrs;
    let role = resolve_role(data);
    let mut node = Node::new(role);

    if let Some(label) = &data.attrs.label {
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
        match (&data.attrs.label, &data.attrs.value) {
            (Some(label), Some(value)) => {
                node.set_value(format!("{}\n{}", label.as_str(), value.as_str()));
            }
            (Some(text), None) | (None, Some(text)) => node.set_value(text.as_str()),
            (None, None) => {}
        }
    } else if let Some(value) = &data.attrs.value {
        node.set_value(value.as_str());
    }
    if let Some(range) = data.attrs.numeric_range {
        node.set_numeric_value(range.value());
        node.set_min_numeric_value(range.min());
        node.set_max_numeric_value(range.max());
        node.set_numeric_value_step(range.step());
        // AccessKit reports a slider writable unless told otherwise
        // (`accesskit_consumer` 0.39, `Node::is_read_only`): a range no write
        // can reach is marked read-only, so a platform write is refused
        // rather than reported and then dropped by the owner.
        if !value_writable(data, role) {
            node.set_read_only();
        }
    }
    // FLUI's `hint` is supplementary prose about what a control does, which is
    // what AccessKit calls a description.
    if let Some(hint) = &data.attrs.hint {
        node.set_description(hint.as_str());
    }
    if let Some(tooltip) = &data.attrs.tooltip {
        node.set_tooltip(tooltip.as_str());
    }
    if let Some(placeholder) = &data.attrs.placeholder {
        node.set_placeholder(placeholder.as_str());
    }
    // AccessKit's level is zero-based where `aria-level` is one-based.
    if role == Role::Heading
        && let Some(level) = data.attrs.heading_level
    {
        node.set_level(usize::from(level.get() - 1));
    }
    if let Some(direction) = data.attrs.text_direction {
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

    match data
        .attrs
        .scroll_axis
        .unwrap_or(flui_foundation::geometry::Axis::Vertical)
    {
        flui_foundation::geometry::Axis::Vertical => {
            if let Some(position) = data.attrs.scroll_position {
                node.set_scroll_y(position);
            }
            if let Some(max) = data.attrs.scroll_extent_max {
                node.set_scroll_y_max(max);
            }
            if let Some(min) = data.attrs.scroll_extent_min {
                node.set_scroll_y_min(min);
            }
        }
        flui_foundation::geometry::Axis::Horizontal => {
            if let Some(position) = data.attrs.scroll_position {
                node.set_scroll_x(position);
            }
            if let Some(max) = data.attrs.scroll_extent_max {
                node.set_scroll_x_max(max);
            }
            if let Some(min) = data.attrs.scroll_extent_min {
                node.set_scroll_x_min(min);
            }
        }
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
    if let Some(index) = data.attrs.index_in_parent
        && index >= 0
    {
        node.set_position_in_set(index as usize + 1);
    }
    if let Some(count) = data.attrs.scroll_child_count
        && count >= 0
    {
        node.set_size_of_set(count as usize);
    }

    apply_state(&mut node, data.attrs);
    apply_actions(
        &mut node,
        data.actions,
        data.attrs.states.expanded,
        value_writable(data, role),
    );

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
///
/// The root also carries the window's device pixel ratio as a scale
/// transform. FLUI's tree stays in logical pixels, but every AccessKit
/// adapter reads the transformed bounds as physical pixels relative to the
/// window's client area (`accesskit::Node::transform`; the Windows adapter
/// adds them to the client origin as they are, the AppKit adapter divides
/// them by `backingScaleFactor`). One transform on the root scales every
/// descendant, so no rect is scaled twice and a ratio change republishes
/// only the root.
#[must_use]
pub(crate) fn to_published_node(
    data: &PublishView<'_>,
    is_root: bool,
    device_pixel_ratio: f64,
    anchors: &AnchorIndex,
) -> Node {
    let mut node = to_node(data);
    apply_relations(&mut node, data, anchors);
    #[expect(
        clippy::float_cmp,
        reason = "exactly 1.0 is the identity AccessKit asks to leave unset"
    )]
    if is_root && device_pixel_ratio != 1.0 {
        node.set_transform(Affine::scale(device_pixel_ratio));
    }
    // Only a root no role reached: an explicit role AccessKit can only
    // express as a container (`DragHandle`, `HotKey`) keeps that container.
    if is_root && data.attrs.role == SemanticsRole::None && node.role() == Role::GenericContainer {
        node.set_role(Role::Window);
    }
    node
}

/// The published ids of the nodes declaring each anchor (ADR-0205).
pub(crate) type AnchorIndex = FxHashMap<SemanticsAnchor, SmallVec<[NodeId; 1]>>;

/// Indexes every addressable node's anchors in `nodes`.
pub(crate) fn anchor_index<'a>(nodes: impl IntoIterator<Item = &'a SemanticsNode>) -> AnchorIndex {
    let mut index = AnchorIndex::default();
    for node in nodes {
        let Some(identity) = node.accessibility_id() else {
            continue;
        };
        for anchor in node.config().anchors() {
            index
                .entry(anchor.clone())
                .or_default()
                .push(NodeId(identity.as_u64()));
        }
    }
    index
}

/// Publishes each relation to the nodes its anchor names. A relation whose
/// anchor no published node declares is omitted rather than pointed at a node
/// the adapter does not have.
fn apply_relations(node: &mut Node, data: &PublishView<'_>, anchors: &AnchorIndex) {
    // Sorted, so the published list does not depend on index iteration order
    // and an unchanged relation diffs as unchanged.
    let targets = |kind: SemanticsRelation| -> Vec<NodeId> {
        let mut ids: Vec<NodeId> = data
            .attrs
            .relations
            .iter()
            .filter(|(relation, _)| *relation == kind)
            .filter_map(|(_, anchor)| anchors.get(anchor))
            .flatten()
            .copied()
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    };
    let labelled_by = targets(SemanticsRelation::LabelledBy);
    if !labelled_by.is_empty() {
        node.set_labelled_by(labelled_by);
    }
    let controls = targets(SemanticsRelation::Controls);
    if !controls.is_empty() {
        node.set_controls(controls);
    }
}

/// The node declaring itself focused, if exactly one does.
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
///   [`AccessibilityNodeId`], which
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
/// its focused state, because that is where the framework records it
/// — a caller should not have to re-derive what the tree already states, and a
/// caller that forgets would silently publish the root as focused. Focus falls
/// back to the root only when no node claims it, or when the named node is not
/// in the published set: AccessKit requires a valid target, and pointing at a
/// node the adapter has never seen is worse than pointing at the root.
///
/// Returns `None` for a tree whose root is missing or unaddressable, which
/// cannot produce an applicable update.
///
/// # Units
///
/// The update is in logical pixels (a device pixel ratio of 1). A platform
/// adapter needs physical pixels: publish through a
/// [`SemanticsOwner`](crate::owner::SemanticsOwner) given the window's ratio
/// ([`SemanticsOwner::set_device_pixel_ratio`](crate::owner::SemanticsOwner::set_device_pixel_ratio)).
#[must_use]
pub fn tree_to_update(
    tree: &crate::tree::SemanticsTree,
    focus: Option<flui_foundation::SemanticsId>,
) -> Option<TreeUpdate> {
    tree_to_scaled_update(tree, focus, 1.0)
}

/// [`tree_to_update`] with the root scaled from logical to physical pixels.
pub(crate) fn tree_to_scaled_update(
    tree: &crate::tree::SemanticsTree,
    focus: Option<flui_foundation::SemanticsId>,
    device_pixel_ratio: f64,
) -> Option<TreeUpdate> {
    let stable_id = |id: flui_foundation::SemanticsId| -> Option<NodeId> {
        tree.get(id)
            .and_then(SemanticsNode::accessibility_id)
            .map(|accessibility_id| NodeId(accessibility_id.as_u64()))
    };

    // An aliased identity cannot be expressed: AccessKit requires one node
    // per id, and its consumer panics on an id two parents list.
    if tree.has_aliased_identities() {
        return None;
    }

    // AccessKit assumes a tree and rejects a full update with a node no
    // parent reaches: the update is built from the rooted walk, and a cycle
    // or a shared child — assembly defects — publish nothing rather than
    // reach an adapter.
    let Some(rooted) = tree.rooted_nodes() else {
        tracing::error!("BUG: the semantics tree has a cycle or a shared child; not published");
        return None;
    };
    let root_node_id = stable_id(tree.root()?)?;
    let rooted: Vec<&SemanticsNode> = rooted.into_iter().filter_map(|id| tree.get(id)).collect();
    let anchors = anchor_index(rooted.iter().copied());

    let nodes: Vec<(NodeId, Node)> = rooted
        .into_iter()
        .filter_map(|node| {
            // `publish_view` skips an unaddressable node (returns `None`),
            // resolves the children into the same stable space with the
            // identical skip rule, and works from the reference already in
            // hand — no second arena lookup per node on the publish path.
            let data = tree.publish_view(node)?;
            let node_id = NodeId(data.id.as_u64());
            Some((
                node_id,
                to_published_node(&data, node_id == root_node_id, device_pixel_ratio, &anchors),
            ))
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
    use crate::properties::HeadingLevel;
    use crate::tree::SemanticsTree;

    /// Childless translation of a node with no actions; child wiring is
    /// covered by the tree-level tests.
    fn translate(attrs: &SemanticsAttributes) -> Node {
        translate_with(attrs, 0)
    }

    /// Childless translation of a node with these effective action bits.
    fn translate_with(attrs: &SemanticsAttributes, actions: u64) -> Node {
        to_node(&PublishView {
            id: AccessibilityNodeId::from(render_id(1)),
            attrs,
            actions,
            rect: flui_foundation::geometry::Rect::ZERO,
            children: SmallVec::new(),
        })
    }

    /// A render identity whose packed value is deliberately unequal to any
    /// plausible arena position, so a test cannot pass by coincidence.
    fn render_id(index: u32) -> flui_foundation::RenderId {
        flui_foundation::RenderId::new_gen(
            index,
            core::num::NonZeroU32::new(7).expect("fixture generation is non-zero"),
        )
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
            let expanded = match inbound {
                accesskit::Action::Expand => Some(false),
                accesskit::Action::Collapse => Some(true),
                _ => None,
            };
            let attrs = SemanticsAttributes {
                states: States {
                    expanded,
                    ..States::default()
                },
                ..Default::default()
            };
            assert!(
                translate_with(&attrs, routed.value()).supports_action(inbound),
                "{inbound:?} routes inbound to {routed:?}, whose outbound translation no \
                 longer advertises {inbound:?} — the two tables have drifted",
            );
        }
    }

    /// A numeric range is writable on the platform, and advertises
    /// `SetValue`, exactly when a handler some platform write reaches exists:
    /// the numeric handler for a number (UIA `RangeValue`), whatever text the
    /// node shows, and the text handler for a string when the node shows text
    /// (UIA `Value`). Any other range is read-only, so a UIA `SetValue` is
    /// refused rather than reported and dropped.
    #[test]
    fn a_numeric_range_is_writable_only_through_the_handler_a_write_reaches() {
        let range = crate::NumericRange::new(5.0, 0.0, 10.0, 1.0).expect("finite fixture");
        let set_text = SemanticsAction::SetText.value();
        let set_number = SemanticsAction::SetNumericValue.value();
        // (row, text value, actions, writable)
        let rows: &[(&str, Option<&str>, u64, bool)] = &[
            ("numeric_handler", None, set_number, true),
            ("both_handlers", None, set_text | set_number, true),
            ("no_handler", None, 0, false),
            ("text_handler_without_text", None, set_text, false),
            (
                "increase_only",
                None,
                SemanticsAction::Increase.value(),
                false,
            ),
            ("text_value_and_text_handler", Some("50%"), set_text, true),
            (
                "text_value_and_numeric_handler",
                Some("50%"),
                set_number,
                true,
            ),
            (
                "text_value_and_both_handlers",
                Some("50%"),
                set_text | set_number,
                true,
            ),
            ("text_value_and_no_handler", Some("50%"), 0, false),
        ];
        let failures: Vec<_> = rows
            .iter()
            .filter(|&&(_, value, actions, writable)| {
                let node = translate_with(
                    &SemanticsAttributes {
                        value: value.map(Into::into),
                        numeric_range: Some(range),
                        ..Default::default()
                    },
                    actions,
                );
                node.is_read_only() == writable
                    || node.supports_action(accesskit::Action::SetValue) != writable
            })
            .map(|&(name, ..)| name)
            .collect();
        assert!(
            failures.is_empty(),
            "range writability rows failed: {failures:?}"
        );
        assert!(
            !translate(&SemanticsAttributes::default()).is_read_only(),
            "a node without a range is not made read-only"
        );
    }

    /// The role mapping's only pin. `SemanticsRole` is `#[non_exhaustive]` and
    /// lives in flui-protocol, so `explicit_role` ends in a wildcard: deleting
    /// any arm there sends that role to `None` (static text or a container)
    /// and nothing but this test notices.
    #[test]
    fn roles_and_checkbox_states_translate_to_accesskit() {
        let mut generic = Vec::new();
        for &role in SemanticsRole::ALL {
            let mapped = explicit_role(role);
            if role == SemanticsRole::None {
                assert_eq!(mapped, None, "`none` declares no role");
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
        for (toggled, expected) in [
            (Toggled::Off, accesskit::Toggled::False),
            (Toggled::On, accesskit::Toggled::True),
            (Toggled::Mixed, accesskit::Toggled::Mixed),
        ] {
            assert_eq!(
                translate(&SemanticsAttributes {
                    role: SemanticsRole::CheckBox,
                    states: States {
                        toggled: Some(toggled),
                        ..States::default()
                    },
                    ..Default::default()
                })
                .toggled(),
                Some(expected),
                "tristate must not collapse to checked/unchecked"
            );
        }
    }

    /// A form control's required and invalid states and its placeholder reach
    /// AccessKit, and a control without them publishes none.
    #[test]
    fn form_states_reach_accesskit() {
        let plain = translate(&SemanticsAttributes {
            role: SemanticsRole::TextInput,
            ..Default::default()
        });
        assert!(!plain.is_required());
        assert_eq!(plain.invalid(), None);
        assert_eq!(plain.placeholder(), None);

        let node = translate(&SemanticsAttributes {
            role: SemanticsRole::TextInput,
            states: States {
                required: Some(true),
                invalid: Some(true),
                ..States::default()
            },
            placeholder: Some("name@example.com".into()),
            ..Default::default()
        });
        assert!(node.is_required());
        assert_eq!(node.invalid(), Some(accesskit::Invalid::True));
        assert_eq!(node.placeholder(), Some("name@example.com"));
    }

    /// A live region announces at the politeness it was given; without one,
    /// an alert is assertive and a status polite, as their ARIA roles imply.
    #[test]
    fn live_regions_announce_at_their_politeness() {
        use Assertiveness::{Assertive, Polite};
        use accesskit::Live;
        let table: &[(Option<Assertiveness>, SemanticsRole, Option<Live>)] = &[
            (None, SemanticsRole::None, None),
            (Some(Polite), SemanticsRole::None, Some(Live::Polite)),
            (Some(Assertive), SemanticsRole::None, Some(Live::Assertive)),
            (None, SemanticsRole::Alert, Some(Live::Assertive)),
            (None, SemanticsRole::Status, Some(Live::Polite)),
            (Some(Polite), SemanticsRole::Alert, Some(Live::Polite)),
        ];
        for &(live, role, expected) in table {
            let node = translate(&SemanticsAttributes {
                live,
                role,
                ..Default::default()
            });
            assert_eq!(node.live(), expected, "{live:?} {role}");
        }
    }

    /// A heading is AccessKit's `Heading`, never its `Header` landmark, and its
    /// one-based level reaches AccessKit's zero-based one.
    #[test]
    fn a_header_publishes_a_heading_at_its_level() {
        let unleveled = translate(&SemanticsAttributes {
            role: SemanticsRole::Heading,
            ..Default::default()
        });
        assert_eq!(unleveled.role(), Role::Heading);
        assert_eq!(unleveled.level(), None);

        for (level, zero_based) in [(HeadingLevel::H1, 0), (HeadingLevel::H6, 5)] {
            let node = translate(&SemanticsAttributes {
                role: SemanticsRole::Heading,
                heading_level: Some(level),
                ..Default::default()
            });
            assert_eq!(node.role(), Role::Heading, "{level:?}");
            assert_eq!(node.level(), Some(zero_based), "{level:?}");
        }

        // A level on another role is not published.
        let tab = translate(&SemanticsAttributes {
            heading_level: Some(HeadingLevel::H2),
            role: SemanticsRole::Tab,
            ..Default::default()
        });
        assert_eq!(tab.role(), Role::Tab);
        assert_eq!(tab.level(), None);
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
