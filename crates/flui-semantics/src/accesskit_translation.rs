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

use crate::action::{
    ActionCall, ActionDecodeError, CustomActionId, ScrollUnit, SemanticsAction, TextSelection,
    UnroutedAction,
};
use crate::applicability::prohibits_name;
use crate::attributes::SemanticsAttributes;
use crate::collection::{Collection, SetSize, SortDirection};
use crate::identity::AccessibilityNodeId;
use crate::node::SemanticsNode;
use crate::properties::AttributedString;
use crate::relation::{SemanticsAnchor, SemanticsRelation};
use crate::role::{Assertiveness, SemanticsRole};
use crate::states::{AutoComplete, Current, HasPopup, Invalid, States, Toggled};
use flui_foundation::geometry::Axis;
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
    bits & action.value() != 0
}

/// The AccessKit role for one node.
///
/// The node's role when it declares one. Otherwise a node that contains a set
/// whose size is declared (a lazy list's scroll container) is a [`Role::List`]:
/// the size is read from the nearest ancestor the consumer keeps, and it keeps
/// no [`Role::GenericContainer`]. A node that carries a label (a plain `Text`,
/// a labelled `Semantics` wrapper) is a [`Role::Label`] — static text — and a
/// node with neither becomes [`Role::GenericContainer`], present in the tree
/// for structure but making no claim about what it is.
///
/// The distinction is load-bearing at the OS boundary: AccessKit's consumer
/// filter (`accesskit_consumer::common_filter`) drops `GenericContainer` nodes
/// from what an assistive technology sees, so a labelled text that resolved to
/// it was invisible to VoiceOver — observed on 2026-09-22 through
/// a macOS accessibility client, where the counter's two `Text`s were absent from the
/// `AXUIElement` tree while its button was present.
#[must_use]
pub(crate) fn resolve_role(data: &PublishView<'_>) -> Role {
    if let Some(role) = explicit_role(data.attrs.effective_role()) {
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
        SemanticsRole::SpinButton => Role::SpinButton,
        SemanticsRole::ComboBox => Role::ComboBox,
        SemanticsRole::ScrollBar => Role::ScrollBar,
        // ARIA's separator is AccessKit's splitter, as in Chromium.
        SemanticsRole::Separator => Role::Splitter,
        SemanticsRole::TextInput => Role::TextInput,
        SemanticsRole::MultilineTextInput => Role::MultilineTextInput,
        SemanticsRole::PasswordInput => Role::PasswordInput,
        SemanticsRole::SearchInput => Role::SearchInput,
        SemanticsRole::EmailInput => Role::EmailInput,
        SemanticsRole::NumberInput => Role::NumberInput,
        SemanticsRole::PhoneNumberInput => Role::PhoneNumberInput,
        SemanticsRole::UrlInput => Role::UrlInput,
        SemanticsRole::DateInput => Role::DateInput,
        SemanticsRole::TimeInput => Role::TimeInput,
        SemanticsRole::ListBox => Role::ListBox,
        SemanticsRole::ListBoxOption => Role::ListBoxOption,
        SemanticsRole::RadioGroup => Role::RadioGroup,
        SemanticsRole::Menu => Role::Menu,
        SemanticsRole::MenuBar => Role::MenuBar,
        SemanticsRole::MenuItem => Role::MenuItem,
        SemanticsRole::MenuItemCheckbox => Role::MenuItemCheckBox,
        SemanticsRole::MenuItemRadio => Role::MenuItemRadio,
        SemanticsRole::Tab => Role::Tab,
        SemanticsRole::TabList => Role::TabList,
        SemanticsRole::TabPanel => Role::TabPanel,
        SemanticsRole::Tree => Role::Tree,
        SemanticsRole::TreeItem => Role::TreeItem,
        SemanticsRole::TreeGrid => Role::TreeGrid,
        SemanticsRole::Grid => Role::Grid,
        SemanticsRole::GridCell => Role::GridCell,
        SemanticsRole::Toolbar => Role::Toolbar,
        SemanticsRole::Table => Role::Table,
        SemanticsRole::Row => Role::Row,
        SemanticsRole::RowGroup => Role::RowGroup,
        SemanticsRole::Cell => Role::Cell,
        SemanticsRole::ColumnHeader => Role::ColumnHeader,
        SemanticsRole::RowHeader => Role::RowHeader,
        SemanticsRole::List => Role::List,
        SemanticsRole::ListItem => Role::ListItem,
        // A heading, not AccessKit's `Header`: that is HTML's `<header>`
        // landmark, which `accesskit_windows` 0.35 reports as a `Group` with
        // the `banner` landmark type, so a title published as one never
        // reached heading navigation.
        SemanticsRole::Heading => Role::Heading,
        SemanticsRole::Paragraph => Role::Paragraph,
        SemanticsRole::Image => Role::Image,
        SemanticsRole::Figure => Role::Figure,
        SemanticsRole::Caption => Role::Caption,
        SemanticsRole::Article => Role::Article,
        SemanticsRole::Feed => Role::Feed,
        SemanticsRole::Group => Role::Group,
        SemanticsRole::Note => Role::Note,
        SemanticsRole::Blockquote => Role::Blockquote,
        SemanticsRole::Code => Role::Code,
        SemanticsRole::Emphasis => Role::Emphasis,
        SemanticsRole::Strong => Role::Strong,
        SemanticsRole::Insertion => Role::ContentInsertion,
        SemanticsRole::Deletion => Role::ContentDeletion,
        SemanticsRole::Term => Role::Term,
        SemanticsRole::Definition => Role::Definition,
        SemanticsRole::Time => Role::Time,
        SemanticsRole::Math => Role::Math,
        SemanticsRole::Meter => Role::Meter,
        SemanticsRole::ProgressBar => Role::ProgressIndicator,
        SemanticsRole::Tooltip => Role::Tooltip,
        SemanticsRole::Document => Role::Document,
        SemanticsRole::Application => Role::Application,
        SemanticsRole::Banner => Role::Banner,
        SemanticsRole::Main => Role::Main,
        SemanticsRole::Navigation => Role::Navigation,
        SemanticsRole::Complementary => Role::Complementary,
        SemanticsRole::ContentInfo => Role::ContentInfo,
        SemanticsRole::Search => Role::Search,
        SemanticsRole::Form => Role::Form,
        SemanticsRole::Region => Role::Region,
        SemanticsRole::Alert => Role::Alert,
        SemanticsRole::Status => Role::Status,
        SemanticsRole::Log => Role::Log,
        SemanticsRole::Marquee => Role::Marquee,
        SemanticsRole::Timer => Role::Timer,
        SemanticsRole::Dialog => Role::Dialog,
        SemanticsRole::AlertDialog => Role::AlertDialog,
        SemanticsRole::Window => Role::Window,
        SemanticsRole::Pane => Role::Pane,
        SemanticsRole::ScrollView => Role::ScrollView,
        // Declared to mean nothing: a structural container, never static
        // text, whatever label it carries.
        SemanticsRole::Generic | SemanticsRole::Presentation => Role::GenericContainer,
        // `SemanticsRole::None`, which declares no role. SemanticsRole is
        // non_exhaustive, so this arm would also take a role added later; the
        // pin is `every_role_maps_to_accesskit`, not this match.
        _ => return None,
    })
}

/// How a node's changes are announced: its declared politeness, else what its
/// role implies (ARIA 1.2: `assertive` for an alert, `polite` for a status or
/// a log, `off` for a timer or marquee), else not at all.
fn live_politeness(live: Option<Assertiveness>, role: SemanticsRole) -> Option<accesskit::Live> {
    match (live, role) {
        (Some(Assertiveness::Assertive), _) | (None, SemanticsRole::Alert) => {
            Some(accesskit::Live::Assertive)
        }
        (Some(Assertiveness::Polite), _) | (None, SemanticsRole::Status | SemanticsRole::Log) => {
            Some(accesskit::Live::Polite)
        }
        // ARIA's implicit `off`, published: an absent setting would inherit an
        // enclosing region's and announce every tick.
        (None, SemanticsRole::Timer | SemanticsRole::Marquee) => Some(accesskit::Live::Off),
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
        current,
        has_popup,
        auto_complete,
        orientation,
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
    // A declared `False` publishes nothing: AccessKit's absence is ARIA's
    // `false`. The declaration still decided merging.
    match invalid {
        Some(Invalid::True) => node.set_invalid(accesskit::Invalid::True),
        Some(Invalid::Grammar) => node.set_invalid(accesskit::Invalid::Grammar),
        Some(Invalid::Spelling) => node.set_invalid(accesskit::Invalid::Spelling),
        Some(Invalid::False) | None => {}
    }
    if let Some(current) = current {
        node.set_aria_current(match current {
            Current::Page => accesskit::AriaCurrent::Page,
            Current::Step => accesskit::AriaCurrent::Step,
            Current::Location => accesskit::AriaCurrent::Location,
            Current::Date => accesskit::AriaCurrent::Date,
            Current::Time => accesskit::AriaCurrent::Time,
            Current::True => accesskit::AriaCurrent::True,
        });
    }
    if let Some(popup) = has_popup {
        node.set_has_popup(match popup {
            HasPopup::Menu => accesskit::HasPopup::Menu,
            HasPopup::ListBox => accesskit::HasPopup::Listbox,
            HasPopup::Tree => accesskit::HasPopup::Tree,
            HasPopup::Grid => accesskit::HasPopup::Grid,
            HasPopup::Dialog => accesskit::HasPopup::Dialog,
        });
    }
    if let Some(auto_complete) = auto_complete {
        node.set_auto_complete(match auto_complete {
            AutoComplete::Inline => accesskit::AutoComplete::Inline,
            AutoComplete::List => accesskit::AutoComplete::List,
            AutoComplete::Both => accesskit::AutoComplete::Both,
        });
    }
    if let Some(orientation) = orientation {
        node.set_orientation(match orientation {
            Axis::Horizontal => accesskit::Orientation::Horizontal,
            Axis::Vertical => accesskit::Orientation::Vertical,
        });
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
    (text_reaches && has_action(data.actions, SemanticsAction::SetValue))
        || (data.attrs.numeric_range.is_some()
            && has_action(data.actions, SemanticsAction::SetNumericValue))
}

/// The AccessKit action a FLUI action is advertised as, for the actions that
/// publish one-to-one. The value writes and the disclosure transitions depend
/// on the node's state and are decided in [`apply_actions`].
const fn advertised_action(action: SemanticsAction) -> Option<accesskit::Action> {
    Some(match action {
        SemanticsAction::Click => accesskit::Action::Click,
        SemanticsAction::Focus => accesskit::Action::Focus,
        SemanticsAction::Blur => accesskit::Action::Blur,
        SemanticsAction::Increment => accesskit::Action::Increment,
        SemanticsAction::Decrement => accesskit::Action::Decrement,
        SemanticsAction::ScrollUp => accesskit::Action::ScrollUp,
        SemanticsAction::ScrollDown => accesskit::Action::ScrollDown,
        SemanticsAction::ScrollLeft => accesskit::Action::ScrollLeft,
        SemanticsAction::ScrollRight => accesskit::Action::ScrollRight,
        SemanticsAction::ScrollIntoView => accesskit::Action::ScrollIntoView,
        SemanticsAction::SetScrollOffset => accesskit::Action::SetScrollOffset,
        SemanticsAction::SetTextSelection => accesskit::Action::SetTextSelection,
        SemanticsAction::ReplaceSelectedText => accesskit::Action::ReplaceSelectedText,
        SemanticsAction::ShowContextMenu => accesskit::Action::ShowContextMenu,
        SemanticsAction::ShowTooltip => accesskit::Action::ShowTooltip,
        SemanticsAction::HideTooltip => accesskit::Action::HideTooltip,
        _ => return None,
    })
}

/// The AccessKit action a published node must offer for a request for
/// `action` to have come from it: a numeric write arrives as `SetValue`, and a
/// disclosure transition as itself whichever handler serves it.
pub(crate) const fn offered_as(action: SemanticsAction) -> Option<accesskit::Action> {
    match action {
        SemanticsAction::Expand => Some(accesskit::Action::Expand),
        SemanticsAction::Collapse => Some(accesskit::Action::Collapse),
        SemanticsAction::SetValue | SemanticsAction::SetNumericValue => {
            Some(accesskit::Action::SetValue)
        }
        SemanticsAction::CustomAction => Some(accesskit::Action::CustomAction),
        other => advertised_action(other),
    }
}

/// Translate the supported actions onto the AccessKit node.
///
/// Every FLUI action has an AccessKit counterpart (ADR-0203). Most publish
/// one-to-one ([`advertised_action`]); three depend on the node's state:
///
/// - `CustomAction` is advertised with the custom actions it can run, and
///   only when there is one: an adapter offers and returns their ids.
///
/// - `SetValue` is advertised when some write the platform can send has a
///   handler ([`value_writable`]): a string reaches the text handler and a
///   number the numeric one.
/// - A node with an expanded state advertises only the one transition its
///   state allows: `Expand` while collapsed, `Collapse` while expanded. It
///   advertises that transition when it registers the matching discrete
///   action, or, when it registers neither discrete action, when it has a
///   click handler; the owner then routes the request to that handler
///   ([`tap_disclosure_transition`](crate::action::tap_disclosure_transition)).
///   AccessKit does not count a node with an expanded state as invocable
///   (`accesskit_consumer` 0.39, `Node::is_invocable`), so without this a
///   click-only expandable node could be neither invoked nor expanded.
fn apply_actions(node: &mut Node, actions: u64, expanded: Option<bool>, value_writable: bool) {
    for &action in SemanticsAction::ALL {
        if has_action(actions, action)
            && let Some(advertised) = advertised_action(action)
        {
            node.add_action(advertised);
        }
    }
    if let Some(expanded) = expanded {
        let transition = if expanded {
            SemanticsAction::Collapse
        } else {
            SemanticsAction::Expand
        };
        let fallback = crate::action::tap_disclosure_transition(actions, Some(expanded));
        if has_action(actions, transition) || fallback == Some(transition) {
            node.add_action(if expanded {
                accesskit::Action::Collapse
            } else {
                accesskit::Action::Expand
            });
        }
    }
    if value_writable {
        node.add_action(accesskit::Action::SetValue);
    }
}

/// Translate an inbound AccessKit action back into the FLUI action space.
///
/// The inverse of `apply_actions`'s table — keep the two adjacent and in
/// agreement, because an action advertised outbound but unroutable inbound is
/// a control assistive technology can see and press but that does nothing.
///
/// `SetValue` maps to the text write when only the action is known;
/// [`semantics_action_request_for`] reads the payload and routes a number to
/// `SetNumericValue`. `None` for the two AccessKit actions FLUI does not route:
/// `ScrollToPoint`, whose point is in window pixels the semantics layer
/// cannot convert, and `SetSequentialFocusNavigationStartingPoint`, which
/// belongs to focus management rather than to a node.
///
/// The match names every AccessKit action, with no wildcard arm:
/// `accesskit::Action` is not `#[non_exhaustive]`, so an upstream release that
/// adds one stops this compiling until someone decides whether FLUI routes it
/// (ADR-0089 §3).
#[must_use]
pub fn semantics_action_for(action: accesskit::Action) -> Option<SemanticsAction> {
    route(action).ok()
}

/// The FLUI action an AccessKit action runs, or the reason it runs none.
fn route(action: accesskit::Action) -> Result<SemanticsAction, UnroutedAction> {
    Ok(match action {
        accesskit::Action::Click => SemanticsAction::Click,
        accesskit::Action::Focus => SemanticsAction::Focus,
        accesskit::Action::Blur => SemanticsAction::Blur,
        accesskit::Action::Expand => SemanticsAction::Expand,
        accesskit::Action::Collapse => SemanticsAction::Collapse,
        accesskit::Action::Increment => SemanticsAction::Increment,
        accesskit::Action::Decrement => SemanticsAction::Decrement,
        accesskit::Action::ScrollUp => SemanticsAction::ScrollUp,
        accesskit::Action::ScrollDown => SemanticsAction::ScrollDown,
        accesskit::Action::ScrollLeft => SemanticsAction::ScrollLeft,
        accesskit::Action::ScrollRight => SemanticsAction::ScrollRight,
        accesskit::Action::ScrollIntoView => SemanticsAction::ScrollIntoView,
        accesskit::Action::SetScrollOffset => SemanticsAction::SetScrollOffset,
        accesskit::Action::SetTextSelection => SemanticsAction::SetTextSelection,
        accesskit::Action::ReplaceSelectedText => SemanticsAction::ReplaceSelectedText,
        accesskit::Action::SetValue => SemanticsAction::SetValue,
        accesskit::Action::ShowContextMenu => SemanticsAction::ShowContextMenu,
        accesskit::Action::ShowTooltip => SemanticsAction::ShowTooltip,
        accesskit::Action::HideTooltip => SemanticsAction::HideTooltip,
        accesskit::Action::CustomAction => SemanticsAction::CustomAction,
        accesskit::Action::ScrollToPoint => return Err(UnroutedAction::ScrollToPoint),
        accesskit::Action::SetSequentialFocusNavigationStartingPoint => {
            return Err(UnroutedAction::SetSequentialFocusNavigationStartingPoint);
        }
    })
}

/// Decode a platform request into the call its handler receives.
///
/// The payload must fit the action: a text for `SetValue` and
/// `ReplaceSelectedText`, a number for a numeric `SetValue`, a point for
/// `SetScrollOffset`, a selection within the target node for
/// `SetTextSelection` (FLUI publishes no text runs yet, so a selection whose
/// ends lie on another node has no meaning here), an id for `CustomAction`.
/// A directional scroll carries its `ScrollUnit` (`Page` when the request has none);
/// a reveal's `ScrollHint` is a preference and is not passed on. Any other payload on an action
/// that takes none is refused. Validity against the current node (a number
/// inside the range) is the owner's check at dispatch.
///
/// # Errors
///
/// [`ActionDecodeError`] names the target, tree, action or payload that did not fit.
pub fn semantics_action_request_for(
    request: &accesskit::ActionRequest,
) -> Result<crate::SemanticsActionRequest, ActionDecodeError> {
    use accesskit::ActionData;

    if request.target_tree != TreeId::ROOT {
        return Err(ActionDecodeError::UnknownTree(request.target_tree));
    }
    let node_id = crate::AccessibilityNodeId::from_u64(request.target_node.0)
        .ok_or(ActionDecodeError::UnknownTarget(request.target_node.0))?;
    let action = route(request.action).map_err(ActionDecodeError::Unrouted)?;
    let mismatched = ActionDecodeError::MismatchedPayload { action };
    let call = match (action, &request.data) {
        (SemanticsAction::SetValue, Some(ActionData::Value(text))) => {
            ActionCall::SetValue(text.to_string())
        }
        (SemanticsAction::SetValue, Some(ActionData::NumericValue(value))) => {
            ActionCall::SetNumericValue(*value)
        }
        (SemanticsAction::ReplaceSelectedText, Some(ActionData::Value(text))) => {
            ActionCall::ReplaceSelectedText(text.to_string())
        }
        // A non-finite offset would poison the receiving scroll position.
        (SemanticsAction::SetScrollOffset, Some(ActionData::SetScrollOffset(point))) => {
            if !(point.x.is_finite() && point.y.is_finite()) {
                return Err(mismatched);
            }
            ActionCall::SetScrollOffset(flui_foundation::geometry::Offset::new(point.x, point.y))
        }
        (SemanticsAction::SetTextSelection, Some(ActionData::SetTextSelection(selection))) => {
            if selection.anchor.node != request.target_node
                || selection.focus.node != request.target_node
            {
                return Err(mismatched);
            }
            ActionCall::SetTextSelection(TextSelection {
                anchor: selection.anchor.character_index,
                focus: selection.focus.character_index,
            })
        }
        (SemanticsAction::CustomAction, Some(ActionData::CustomAction(id))) => {
            ActionCall::CustomAction(CustomActionId::from_published(*id).ok_or(mismatched)?)
        }
        (
            SemanticsAction::ScrollUp
            | SemanticsAction::ScrollDown
            | SemanticsAction::ScrollLeft
            | SemanticsAction::ScrollRight,
            Some(ActionData::ScrollUnit(unit)),
        ) => {
            let unit = match unit {
                accesskit::ScrollUnit::Item => ScrollUnit::Item,
                accesskit::ScrollUnit::Page => ScrollUnit::Page,
            };
            match action {
                SemanticsAction::ScrollUp => ActionCall::ScrollUp(unit),
                SemanticsAction::ScrollDown => ActionCall::ScrollDown(unit),
                SemanticsAction::ScrollLeft => ActionCall::ScrollLeft(unit),
                _ => ActionCall::ScrollRight(unit),
            }
        }
        // A reveal's position hint is a preference FLUI does not pass on: it
        // reveals with the least movement.
        (SemanticsAction::ScrollIntoView, Some(ActionData::ScrollHint(_))) | (_, None) => {
            ActionCall::without_payload(action)
                .ok_or(ActionDecodeError::MissingPayload { action })?
        }
        (_, Some(_)) => return Err(mismatched),
    };
    Ok(crate::SemanticsActionRequest::new(node_id, call))
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
    // Publication reads the view `applicability` keeps: what the declared role
    // does not support is left out here, and the owner reports it.
    let view = crate::applicability::retain_applicable(data.attrs);
    let filtered;
    let data = if let std::borrow::Cow::Owned(attrs) = &view {
        filtered = PublishView {
            id: data.id,
            attrs,
            actions: data.actions,
            rect: data.rect,
            children: data.children.clone(),
        };
        &filtered
    } else {
        data
    };
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
        keyboard_shortcut: _,
        language: _,
        url: _,
        role_description: _,
        identifier: _,
        heading_level: _,
        text_direction: _,
        scroll_position: _,
        scroll_axis: _,
        scroll_extent_max: _,
        scroll_extent_min: _,
        // Published by `apply_collection`.
        collection: _,
        // Resolved against the anchor index by `to_published_node`.
        anchors: _,
        relations: _,
        // Framework-side: merge bookkeeping and traversal order, never
        // published as properties.
        tags: _,
        sort_key: _,
        hint_overrides: _,
        scroll_index: _,
        // Published with the `CustomAction` bit below.
        custom_actions,
        // Unpublished: AccessKit has no adjacent-value or platform-view
        // property, and FLUI publishes no text runs that text lengths describe.
        increased_value: _,
        decreased_value: _,
        platform_view_id: _,
        max_value_length: _,
        current_value_length: _,
    } = data.attrs;
    let role = resolve_role(data);
    let mut node = Node::new(role);

    // ARIA 1.2 forbids a name on some text-level roles (a paragraph, code,
    // emphasis); their label is published as the description below instead.
    if let Some(label) = &data.attrs.label
        && !prohibits_name(data.attrs.role)
    {
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
        if let Some(jump) = range.jump() {
            node.set_numeric_value_jump(jump);
        }
        // AccessKit reports a slider writable unless told otherwise
        // (`accesskit_consumer` 0.39, `Node::is_read_only`): a range no write
        // can reach is marked read-only, so a platform write is refused
        // rather than reported and then dropped by the owner.
        if !value_writable(data, role) {
            node.set_read_only();
        }
    }
    // FLUI's `hint` is supplementary prose about what a control does, which is
    // what AccessKit calls a description. A role ARIA forbids a name keeps its
    // label there too, ahead of the hint, so the text still reaches the user.
    let moved_label = data
        .attrs
        .label
        .as_ref()
        .filter(|_| prohibits_name(data.attrs.role));
    match (moved_label, &data.attrs.hint) {
        (Some(label), Some(hint)) => {
            node.set_description(format!("{}\n{}", label.as_str(), hint.as_str()));
        }
        (Some(text), None) | (None, Some(text)) => node.set_description(text.as_str()),
        (None, None) => {}
    }
    if let Some(tooltip) = &data.attrs.tooltip {
        node.set_tooltip(tooltip.as_str());
    }
    if let Some(placeholder) = &data.attrs.placeholder {
        node.set_placeholder(placeholder.as_str());
    }
    if let Some(shortcut) = &data.attrs.keyboard_shortcut {
        node.set_keyboard_shortcut(shortcut.as_str());
    }
    if let Some(language) = &data.attrs.language {
        node.set_language(language.as_str());
    }
    if let Some(url) = &data.attrs.url {
        node.set_url(url.as_str());
    }
    if let Some(description) = &data.attrs.role_description {
        node.set_role_description(description.as_str());
    }
    // The author's identifier (ADR-0240): adapters map `author_id` to UI
    // Automation's `AutomationId`, `AXIdentifier` and AT-SPI's accessible id,
    // none of which a screen reader speaks.
    if let Some(identifier) = &data.attrs.identifier {
        node.set_author_id(&**identifier);
    }
    // AccessKit's level is zero-based where `aria-level` is one-based.
    if let Some(level) = data.attrs.heading_level {
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

    apply_collection(&mut node, &data.attrs.collection);

    apply_state(&mut node, data.attrs);
    // Custom actions are offered with their ids and labels while user actions
    // are not blocked; every one carries its own handler.
    if has_action(data.actions, SemanticsAction::CustomAction) && !custom_actions.is_empty() {
        node.add_action(accesskit::Action::CustomAction);
        node.set_custom_actions(
            custom_actions
                .iter()
                .enumerate()
                .map(|(index, label)| accesskit::CustomAction {
                    id: CustomActionId::at(index).published(),
                    description: label.to_string(),
                })
                .collect::<Vec<_>>(),
        );
    }
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
/// found through a UI Automation client. The AppKit adapter reads
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
    // Only a root that declares no role and has no name becomes the window.
    if is_root && data.attrs.role == SemanticsRole::None && node.role() == Role::GenericContainer {
        node.set_role(Role::Window);
    }
    node
}

/// The nodes declaring each anchor (ADR-0205).
pub(crate) type AnchorIndex = FxHashMap<SemanticsAnchor, SmallVec<[AnchorTarget; 1]>>;

/// A node an anchor resolves to.
#[derive(Debug, Clone)]
pub(crate) struct AnchorTarget {
    /// Its published id.
    pub(crate) id: NodeId,
    /// The text that names what it labels, as an adapter reads it: a static
    /// text's published value (its label, then its value), anything else's
    /// label. Never a control's value, which names nothing, and a password
    /// field's would leave the protection its own node gives it.
    text: Option<String>,
    /// Whether it publishes no name of its own (a role ARIA forbids one), so
    /// an adapter reading the relation finds no text on it.
    nameless: bool,
}

impl AnchorTarget {
    /// `node` as a relation target, published under `identity`.
    pub(crate) fn of(node: &SemanticsNode, identity: AccessibilityNodeId) -> Self {
        let attrs = node.config().attributes();
        let label = attrs.label.as_ref().map(AttributedString::as_str);
        // A node with no role and a label publishes as static text, named by
        // its value (`to_node`).
        let static_text =
            explicit_role(attrs.role).is_none() && label.is_some_and(|l| !l.is_empty());
        let text = match (label, attrs.value.as_ref().filter(|_| static_text)) {
            (Some(label), Some(value)) => Some(format!("{label}\n{}", value.as_str())),
            (Some(label), None) => Some(label.to_owned()),
            (None, _) => None,
        };
        Self {
            id: NodeId(identity.as_u64()),
            text,
            nameless: prohibits_name(attrs.role),
        }
    }
}

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
                .push(AnchorTarget::of(node, identity));
        }
    }
    index
}

/// AccessKit's names for where a node sits in a set, a tree or a table.
///
/// A screen reader reads "item 12 of 100" from `position_in_set` on the item
/// and `size_of_set` on its container (`size_of_set_from_container`).
/// `position_in_set` is zero-based as the framework's is (the adapters add one:
/// `accesskit_windows` `node.rs`); AccessKit's level, row and column are
/// zero-based where the framework's are the one-based numbers ARIA gives.
///
/// A position the adapters would convert to an `i32` that overflows
/// (`i32::MAX` and up) is dropped rather than published as a nonsensical one,
/// and so are the sizes, counts and spans that do not fit an `i32`. An unknown
/// size, count or row is *not published*: AccessKit has no way to say
/// "unknown", and a number would claim the set ends where the built members do.
fn apply_collection(node: &mut Node, collection: &Collection) {
    let fits = |count: usize| i32::try_from(count).is_ok();
    if let Some(index) = collection.position_in_set
        && fits(index)
        && index != i32::MAX as usize
    {
        node.set_position_in_set(index);
    }
    if let Some(SetSize::Known(count)) = collection.set_size
        && fits(count)
    {
        node.set_size_of_set(count);
    }
    if let Some(level) = collection.level
        && fits(level.get() as usize)
    {
        node.set_level(level.get() as usize - 1);
    }
    if let Some(row) = collection.row
        && fits(row.get())
    {
        node.set_row_index(row.get() - 1);
    }
    if let Some(shape) = collection.table {
        if let SetSize::Known(rows) = shape.rows()
            && fits(rows)
        {
            node.set_row_count(rows);
        }
        if let SetSize::Known(columns) = shape.columns()
            && fits(columns)
        {
            node.set_column_count(columns);
        }
    }
    if let Some(cell) = collection.cell
        && [
            cell.row(),
            cell.column(),
            cell.row_span(),
            cell.column_span(),
        ]
        .iter()
        .all(|value| fits(value.get()))
    {
        // A cell is published whole or not at all: an omitted span reads as
        // AccessKit's default of one, which would misplace a cell whose span
        // cannot be converted. AccessKit counts rows and columns from zero
        // where the framework, like `aria-rowindex`, counts from one.
        node.set_row_index(cell.row().get() - 1);
        node.set_column_index(cell.column().get() - 1);
        // One is AccessKit's default, so it is not published.
        if cell.row_span().get() > 1 {
            node.set_row_span(cell.row_span().get());
        }
        if cell.column_span().get() > 1 {
            node.set_column_span(cell.column_span().get());
        }
    }
    if let Some(direction) = collection.sort {
        node.set_sort_direction(match direction {
            SortDirection::Ascending => accesskit::SortDirection::Ascending,
            SortDirection::Descending => accesskit::SortDirection::Descending,
            SortDirection::Other => accesskit::SortDirection::Other,
        });
    }
    if collection.multiselectable == Some(true) {
        node.set_multiselectable();
    }
}

/// Publishes each relation to the nodes its anchor names. A relation whose
/// anchor no published node declares is omitted rather than pointed at a node
/// the adapter does not have.
fn apply_relations(node: &mut Node, data: &PublishView<'_>, anchors: &AnchorIndex) {
    // Sorted, so the published list does not depend on index iteration order
    // and an unchanged relation diffs as unchanged.
    let targets = |kind: SemanticsRelation| -> Vec<&AnchorTarget> {
        let mut targets: Vec<&AnchorTarget> = data
            .attrs
            .relations
            .iter()
            .filter(|(relation, _)| *relation == kind)
            .filter_map(|(_, anchor)| anchors.get(anchor))
            .flatten()
            .collect();
        targets.sort_unstable_by_key(|target| target.id);
        targets.dedup_by_key(|target| target.id);
        targets
    };
    let ids = |targets: Vec<&AnchorTarget>| -> Vec<NodeId> {
        targets.into_iter().map(|target| target.id).collect()
    };
    let labelled_by = targets(SemanticsRelation::LabelledBy);
    if prohibits_name(data.attrs.role) {
        // A role ARIA forbids a name gets none from the nodes labelling it
        // either: their text leads its description, which every adapter
        // reads (none reads `described_by`, ADR-0205).
        if let Some(text) = labelling_text(&labelled_by) {
            let description = match node.description() {
                Some(own) => format!("{text}\n{own}"),
                None => text,
            };
            node.set_description(description);
        }
    } else if !labelled_by.is_empty() {
        // An adapter names a node from its labelling nodes' own names, and a
        // role ARIA forbids a name has none: when one labels this node, the
        // name is resolved here, from every labelling node's text, unless the
        // node is named directly.
        if node.label().is_none()
            && labelled_by.iter().any(|target| target.nameless)
            && let Some(text) = labelling_text(&labelled_by)
        {
            node.set_label(text);
        }
        node.set_labelled_by(ids(labelled_by));
    }
    let controls = ids(targets(SemanticsRelation::Controls));
    if !controls.is_empty() {
        node.set_controls(controls);
    }
    let described_by = ids(targets(SemanticsRelation::DescribedBy));
    if !described_by.is_empty() {
        node.set_described_by(described_by);
    }
    let details = ids(targets(SemanticsRelation::Details));
    if !details.is_empty() {
        node.set_details(details);
    }
    let flow_to = ids(targets(SemanticsRelation::FlowTo));
    if !flow_to.is_empty() {
        node.set_flow_to(flow_to);
    }
    // AccessKit takes one node for these: the first in identity order, so
    // the choice does not depend on declaration or index iteration order.
    if let Some(target) = targets(SemanticsRelation::ErrorMessage).first() {
        node.set_error_message(target.id);
    }
    if let Some(target) = targets(SemanticsRelation::ActiveDescendant).first() {
        node.set_active_descendant(target.id);
    }
}

/// The text of the nodes labelling another, in order, joined by spaces as an
/// adapter joins them; `None` when none carries any.
fn labelling_text(labelled_by: &[&AnchorTarget]) -> Option<String> {
    let text: Vec<&str> = labelled_by
        .iter()
        .filter_map(|target| target.text.as_deref())
        .collect();
    (!text.is_empty()).then(|| text.join(" "))
}

/// The node declaring itself focused, if exactly one does.
///
/// Two nodes claiming focus is a malformed tree, and guessing between them
/// would make the published focus depend on arena order. Reporting `None` sends
/// focus to the root, which is at least a node the adapter has definitely seen.
fn focused_node(
    mut claiming: impl Iterator<Item = flui_foundation::SemanticsId>,
) -> Option<flui_foundation::SemanticsId> {
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

    // AccessKit assumes a tree and rejects a full update with a node no
    // parent reaches: the update is built from the rooted walk, and a cycle
    // or a shared child — assembly defects — publish nothing rather than
    // reach an adapter.
    let Some(rooted) = tree.rooted_nodes() else {
        tracing::error!("BUG: the semantics tree has a cycle or a shared child; not published");
        return None;
    };
    // An aliased identity cannot be expressed: AccessKit requires one node
    // per id, and its consumer panics on an id two parents list. An
    // unattached alias is left out with the rest of the unrooted nodes.
    if !tree.rooted_identities_are_unique(&rooted) {
        tracing::error!(
            "BUG: two rooted semantics nodes publish one accessibility id; not published"
        );
        return None;
    }
    let root_node_id = stable_id(tree.root()?)?;
    // Focus is claimed only among the nodes this update carries: an
    // unattached claimant must not make a published one ambiguous.
    let focused = focused_node(
        rooted
            .iter()
            .copied()
            .filter(|&id| tree.get(id).is_some_and(|node| node.config().is_focused())),
    );
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
        .or(focused)
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
pub(crate) mod coverage_matrix;

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
    /// A window publishes one tree, so a request for another is not decoded.
    #[test]
    fn a_request_for_another_tree_is_refused() {
        let other = TreeId(accesskit::Uuid::from_u128(1));
        let request = accesskit::ActionRequest {
            action: accesskit::Action::Click,
            target_tree: other,
            target_node: NodeId(AccessibilityNodeId::from(render_id(1)).as_u64()),
            data: None,
        };
        assert_eq!(
            semantics_action_request_for(&request).err(),
            Some(ActionDecodeError::UnknownTree(other))
        );
    }

    /// Every FLUI action publishes as an AccessKit action that routes back to
    /// it (ADR-0203), so a control assistive technology can see and press
    /// always reaches a handler. A node needs the state an action depends on
    /// to advertise it: an expanded state for the transitions, a writable
    /// value for the value writes. `SetNumericValue` publishes as `SetValue`
    /// and comes back through its numeric payload.
    #[test]
    fn every_action_round_trips_through_accesskit() {
        for &action in SemanticsAction::ALL {
            let mut attrs = SemanticsAttributes::default();
            match action {
                SemanticsAction::Expand => attrs.states.expanded = Some(false),
                SemanticsAction::Collapse => attrs.states.expanded = Some(true),
                SemanticsAction::SetNumericValue => {
                    attrs.numeric_range = Some(
                        crate::NumericRange::try_new(0.0, 0.0, 1.0, 0.1).expect("finite fixture"),
                    );
                }
                SemanticsAction::CustomAction => {
                    attrs.custom_actions = smallvec::smallvec!["Archive".into()];
                }
                _ => {}
            }
            let node = translate_with(&attrs, action.value());
            let published: Vec<accesskit::Action> = AK_ACTIONS
                .iter()
                .copied()
                .filter(|&candidate| node.supports_action(candidate))
                .collect();
            let [published] = published[..] else {
                panic!("{action} publishes {published:?}, not exactly one AccessKit action");
            };
            let data = match action {
                SemanticsAction::SetNumericValue => Some(accesskit::ActionData::NumericValue(0.5)),
                _ => sample_payload(action),
            };
            let request = accesskit::ActionRequest {
                action: published,
                target_tree: TreeId::ROOT,
                target_node: NodeId(AccessibilityNodeId::from(render_id(1)).as_u64()),
                data,
            };
            assert_eq!(
                semantics_action_request_for(&request).map(|request| request.action()),
                Ok(action),
                "{action} publishes {published:?}, which does not route back to it"
            );
        }
    }

    /// Every AccessKit action, for [`every_action_round_trips_through_accesskit`]
    /// to find what a node publishes.
    const AK_ACTIONS: &[accesskit::Action] = &[
        accesskit::Action::Click,
        accesskit::Action::Focus,
        accesskit::Action::Blur,
        accesskit::Action::Collapse,
        accesskit::Action::Expand,
        accesskit::Action::CustomAction,
        accesskit::Action::Decrement,
        accesskit::Action::Increment,
        accesskit::Action::HideTooltip,
        accesskit::Action::ShowTooltip,
        accesskit::Action::ReplaceSelectedText,
        accesskit::Action::ScrollDown,
        accesskit::Action::ScrollLeft,
        accesskit::Action::ScrollRight,
        accesskit::Action::ScrollUp,
        accesskit::Action::ScrollIntoView,
        accesskit::Action::ScrollToPoint,
        accesskit::Action::SetScrollOffset,
        accesskit::Action::SetTextSelection,
        accesskit::Action::SetSequentialFocusNavigationStartingPoint,
        accesskit::Action::SetValue,
        accesskit::Action::ShowContextMenu,
    ];

    /// The payload a platform sends with `action`, for the actions that need one.
    fn sample_payload(action: SemanticsAction) -> Option<accesskit::ActionData> {
        let here = NodeId(AccessibilityNodeId::from(render_id(1)).as_u64());
        match action {
            SemanticsAction::SetValue | SemanticsAction::ReplaceSelectedText => {
                Some(accesskit::ActionData::Value("text".into()))
            }
            SemanticsAction::SetScrollOffset => Some(accesskit::ActionData::SetScrollOffset(
                accesskit::Point::new(1.0, 2.0),
            )),
            SemanticsAction::SetTextSelection => Some(accesskit::ActionData::SetTextSelection(
                accesskit::TextSelection {
                    anchor: accesskit::TextPosition {
                        node: here,
                        character_index: 1,
                    },
                    focus: accesskit::TextPosition {
                        node: here,
                        character_index: 3,
                    },
                },
            )),
            SemanticsAction::CustomAction => Some(accesskit::ActionData::CustomAction(1)),
            _ => None,
        }
    }

    /// A platform request decodes into the call its handler receives, or is
    /// refused before any handler runs when its payload does not fit.
    #[test]
    fn platform_requests_decode_to_their_typed_call() {
        use accesskit::{Action as Ak, ActionData as Data};
        let here = NodeId(AccessibilityNodeId::from(render_id(1)).as_u64());
        let elsewhere = NodeId(AccessibilityNodeId::from(render_id(2)).as_u64());
        let decode = |action, data| {
            semantics_action_request_for(&accesskit::ActionRequest {
                action,
                target_tree: TreeId::ROOT,
                target_node: here,
                data,
            })
            .map(|request| request.call)
        };
        let position = |node, character_index| accesskit::TextPosition {
            node,
            character_index,
        };
        let cases: Vec<(Ak, Option<Data>, Result<ActionCall, ActionDecodeError>)> = vec![
            (Ak::Click, None, Ok(ActionCall::Click)),
            (
                Ak::ScrollDown,
                Some(Data::ScrollUnit(accesskit::ScrollUnit::Item)),
                Ok(ActionCall::ScrollDown(ScrollUnit::Item)),
            ),
            (
                Ak::ScrollUp,
                None,
                Ok(ActionCall::ScrollUp(ScrollUnit::Page)),
            ),
            (
                Ak::ScrollIntoView,
                Some(Data::ScrollHint(accesskit::ScrollHint::TopEdge)),
                Ok(ActionCall::ScrollIntoView(None)),
            ),
            (
                Ak::ScrollIntoView,
                None,
                Ok(ActionCall::ScrollIntoView(None)),
            ),
            (
                Ak::SetValue,
                Some(Data::Value("abc".into())),
                Ok(ActionCall::SetValue("abc".into())),
            ),
            (
                Ak::SetValue,
                Some(Data::NumericValue(2.5)),
                Ok(ActionCall::SetNumericValue(2.5)),
            ),
            (
                Ak::SetScrollOffset,
                Some(Data::SetScrollOffset(accesskit::Point::new(3.0, 4.0))),
                Ok(ActionCall::SetScrollOffset(
                    flui_foundation::geometry::Offset::new(3.0, 4.0),
                )),
            ),
            (
                Ak::SetTextSelection,
                Some(Data::SetTextSelection(accesskit::TextSelection {
                    anchor: position(here, 2),
                    focus: position(here, 5),
                })),
                Ok(ActionCall::SetTextSelection(crate::TextSelection {
                    anchor: 2,
                    focus: 5,
                })),
            ),
            (
                Ak::CustomAction,
                Some(Data::CustomAction(9)),
                Ok(ActionCall::CustomAction(CustomActionId::at(8))),
            ),
            (
                Ak::SetValue,
                None,
                Err(ActionDecodeError::MissingPayload {
                    action: SemanticsAction::SetValue,
                }),
            ),
            (
                Ak::CustomAction,
                None,
                Err(ActionDecodeError::MissingPayload {
                    action: SemanticsAction::CustomAction,
                }),
            ),
            (
                Ak::CustomAction,
                Some(Data::CustomAction(0)),
                Err(ActionDecodeError::MismatchedPayload {
                    action: SemanticsAction::CustomAction,
                }),
            ),
            (
                Ak::Click,
                Some(Data::Value("x".into())),
                Err(ActionDecodeError::MismatchedPayload {
                    action: SemanticsAction::Click,
                }),
            ),
            (
                Ak::SetTextSelection,
                Some(Data::SetTextSelection(accesskit::TextSelection {
                    anchor: position(here, 0),
                    focus: position(elsewhere, 1),
                })),
                Err(ActionDecodeError::MismatchedPayload {
                    action: SemanticsAction::SetTextSelection,
                }),
            ),
            (
                Ak::ScrollToPoint,
                Some(Data::ScrollToPoint(accesskit::Point::new(0.0, 0.0))),
                Err(ActionDecodeError::Unrouted(UnroutedAction::ScrollToPoint)),
            ),
            (
                Ak::SetScrollOffset,
                Some(Data::SetScrollOffset(accesskit::Point::new(f64::NAN, 0.0))),
                Err(ActionDecodeError::MismatchedPayload {
                    action: SemanticsAction::SetScrollOffset,
                }),
            ),
            (
                Ak::SetScrollOffset,
                Some(Data::SetScrollOffset(accesskit::Point::new(
                    0.0,
                    f64::INFINITY,
                ))),
                Err(ActionDecodeError::MismatchedPayload {
                    action: SemanticsAction::SetScrollOffset,
                }),
            ),
        ];
        for (action, data, expected) in cases {
            assert_eq!(
                decode(action, data.clone()),
                expected,
                "{action:?} with {data:?}"
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
        let range = crate::NumericRange::try_new(5.0, 0.0, 10.0, 1.0).expect("finite fixture");
        let set_text = SemanticsAction::SetValue.value();
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
                SemanticsAction::Increment.value(),
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
    /// and nothing but this test notices. Two roles publishing one AccessKit
    /// role would also be indistinguishable to every assistive technology.
    #[test]
    fn every_role_maps_to_accesskit() {
        let mut published = Vec::new();
        for &role in SemanticsRole::ALL {
            let mapped = explicit_role(role);
            if role == SemanticsRole::None {
                assert_eq!(mapped, None, "`none` declares no role");
                continue;
            }
            // Declared to mean nothing: a container the consumer lifts its
            // children out of, which no other role publishes.
            if matches!(role, SemanticsRole::Generic | SemanticsRole::Presentation) {
                assert_eq!(mapped, Some(Role::GenericContainer), "{role}");
                continue;
            }
            let mapped = mapped.unwrap_or_else(|| panic!("{role} maps to no AccessKit role"));
            assert_ne!(
                mapped,
                Role::GenericContainer,
                "{role} is a role ADR-0203 admits only with an AccessKit counterpart"
            );
            assert!(
                !published.contains(&mapped),
                "{role} publishes {mapped:?}, which another role already does"
            );
            published.push(mapped);
        }
    }

    /// A checkable node's three states stay three distinct AccessKit values.
    #[test]
    fn toggled_states_stay_distinct_in_accesskit() {
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
                invalid: Some(Invalid::True),
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
            (None, SemanticsRole::Log, Some(Live::Polite)),
            (None, SemanticsRole::Timer, Some(Live::Off)),
            (None, SemanticsRole::Marquee, Some(Live::Off)),
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

    /// A custom action is offered with the id and description an adapter shows
    /// and sends back, and not at all without one: the generic bit alone
    /// names nothing to run.
    #[test]
    fn custom_actions_publish_with_their_ids() {
        let bit = SemanticsAction::CustomAction.value();
        let bare = translate_with(&SemanticsAttributes::default(), bit);
        assert!(!bare.supports_action(accesskit::Action::CustomAction));
        assert_eq!(bare.custom_actions(), &[] as &[accesskit::CustomAction]);

        let node = translate_with(
            &SemanticsAttributes {
                custom_actions: smallvec::smallvec!["Archive".into(), "Pin".into()],
                ..Default::default()
            },
            bit,
        );
        assert!(node.supports_action(accesskit::Action::CustomAction));
        assert_eq!(
            node.custom_actions(),
            &[
                accesskit::CustomAction {
                    id: 1,
                    description: "Archive".into(),
                },
                accesskit::CustomAction {
                    id: 2,
                    description: "Pin".into(),
                },
            ],
            "each action is published under its place in the list"
        );
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
