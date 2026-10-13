//! Semantics configuration for render objects.
//!
//! This module provides the configuration that render objects use to describe
//! their semantic properties for accessibility.

use std::sync::Arc;

use flui_foundation::geometry::Axis;

use rustc_hash::FxHashMap;
use smol_str::SmolStr;

use crate::{
    action::{SemanticsAction, SemanticsActionHandler},
    attributes::SemanticsAttributes,
    properties::{
        AttributedString, CustomSemanticsAction, HeadingLevel, SemanticsHintOverrides,
        SemanticsSortKey, SemanticsTag, TextDirection, UNBLOCKED_USER_ACTIONS_MASK,
        concat_attributed_string,
    },
    relation::{SemanticsAnchor, SemanticsRelation},
    role::{Assertiveness, SemanticsRole},
    states::{States, Toggled},
};

// ============================================================================
// SemanticsConfiguration
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum DescendantSemanticsMerge {
    #[default]
    SeparateNodes,
    MergeIntoThisNode,
}

/// Configuration describing the semantic properties of a render object.
///
/// This is the primary way render objects communicate their accessibility
/// information to the semantics system. Each RenderObject can override
/// `describeSemanticsConfiguration` to fill out this configuration.
///
/// # Example
///
/// ```ignore
/// let mut config = SemanticsConfiguration::new();
/// config.set_label("Submit button");
/// config.set_button(true);
/// config.add_action(SemanticsAction::Tap, Arc::new(|_, _| {
///     println!("Button tapped!");
/// }));
/// ```
#[derive(Default, Clone)]
pub struct SemanticsConfiguration {
    /// Assembly-derived reveal support, without a fabricated node callback.
    has_reveal_ancestor: bool,
    /// Whether a semantic payload setter has touched this configuration.
    ///
    /// Structural assembly directives such as boundary formation, explicit
    /// children, user-action blocking, and tags do not set this bit. The
    /// descendant-merging directive deliberately does.
    has_been_annotated: bool,

    /// Whether the semantic information in this configuration is complete.
    ///
    /// If true, children semantics won't be merged into this node.
    is_semantics_boundary: bool,

    /// Whether pointer-related user actions from this configuration and its
    /// subtree are blocked from assistive-technology dispatch.
    ///
    /// On a standalone boundary this masks the node's exported action bits.
    /// When the configuration contributes to an ancestor through absorption,
    /// the mask applies only to this contributing subtree; actions already
    /// registered by the receiving ancestor remain available unless that
    /// ancestor independently blocks them.
    blocks_user_actions: bool,

    /// Whether contributing children must form explicit semantics nodes.
    ///
    /// Rendering assembly applies this structural directive before immutable
    /// platform snapshots are produced. It is tree-shape input, not adapter
    /// payload, so snapshots expose its resulting child nodes rather than the
    /// directive itself.
    explicit_child_nodes: bool,

    /// How descendant semantics boundary nodes are handled under this
    /// configuration.
    ///
    /// Set alongside `is_semantics_boundary = true` by `RenderMergeSemantics`
    /// (`MergeSemantics` widget). The assembly walk
    /// (`flui-rendering`'s `run_semantics`) honors this by
    /// suppressing every descendant's own boundary decision for the rest of
    /// that subtree, absorbing all descendant configs into this one node.
    descendant_semantics_merge: DescendantSemanticsMerge,

    /// Actions that can be performed on this node.
    actions: FxHashMap<SemanticsAction, SemanticsActionHandler>,

    /// What this node publishes (ADR-0202).
    attributes: SemanticsAttributes,
}

impl PartialEq for SemanticsConfiguration {
    fn eq(&self, other: &Self) -> bool {
        let actions_are_identical = self.actions.len() == other.actions.len()
            && self.actions.iter().all(|(action, handler)| {
                other
                    .actions
                    .get(action)
                    .is_some_and(|other_handler| Arc::ptr_eq(handler, other_handler))
            });

        self.has_reveal_ancestor == other.has_reveal_ancestor
            && self.has_been_annotated == other.has_been_annotated
            && self.is_semantics_boundary == other.is_semantics_boundary
            && self.blocks_user_actions == other.blocks_user_actions
            && self.explicit_child_nodes == other.explicit_child_nodes
            && self.descendant_semantics_merge == other.descendant_semantics_merge
            && actions_are_identical
            && self.attributes == other.attributes
    }
}

impl SemanticsConfiguration {
    /// Creates a new empty configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// What this configuration publishes (ADR-0202).
    #[inline]
    #[must_use]
    pub fn attributes(&self) -> &SemanticsAttributes {
        &self.attributes
    }

    /// Returns whether this configuration contributes semantic payload.
    #[inline]
    pub fn has_been_annotated(&self) -> bool {
        self.has_been_annotated
    }

    /// Whether everything this configuration says is an action handler: no
    /// states, no role, and no label, value, hint or tooltip.
    ///
    /// The shape of the annotation a `GestureDetector` mounts to advertise
    /// its tap to assistive technology, which merges into the control's own
    /// node above it. Diagnostics and widget tests use it to tell that
    /// helper node from the control's wrapper.
    pub fn is_actions_only(&self) -> bool {
        self.has_been_annotated
            && !self.actions.is_empty()
            && self.attributes.states.is_empty()
            && self.attributes.live.is_none()
            && self.attributes.role == SemanticsRole::None
            && self.attributes.label.is_none()
            && self.attributes.value.is_none()
            && self.attributes.hint.is_none()
            && self.attributes.tooltip.is_none()
    }

    /// Whether everything this configuration says is that its node can take
    /// the keyboard focus and whether it has it: only the focusable and
    /// focused states, no actions, role, label, value, hint or tooltip.
    ///
    /// The shape of the annotation a `Focus` widget mounts, which merges into
    /// the control's own node above it; the counterpart of
    /// [`Self::is_actions_only`] for diagnostics and widget tests.
    pub fn is_focus_state_only(&self) -> bool {
        let states = self.attributes.states;
        let focus_only = States {
            focusable: states.focusable,
            focused: states.focused,
            ..States::default()
        };
        self.has_been_annotated
            && !states.is_empty()
            && states == focus_only
            && self.attributes.live.is_none()
            && self.actions.is_empty()
            && self.attributes.role == SemanticsRole::None
            && self.attributes.label.is_none()
            && self.attributes.value.is_none()
            && self.attributes.hint.is_none()
            && self.attributes.tooltip.is_none()
    }

    #[inline]
    fn mark_annotated(&mut self) {
        self.has_been_annotated = true;
    }

    // ========================================================================
    // Boundary Configuration
    // ========================================================================

    /// Returns whether this is a semantics boundary.
    #[inline]
    pub fn is_semantics_boundary(&self) -> bool {
        self.is_semantics_boundary
    }

    /// Sets whether this is a semantics boundary.
    ///
    /// When true, the semantic information from this node won't merge with
    /// its parent, creating a distinct semantics node.
    pub fn set_semantics_boundary(&mut self, value: bool) {
        self.is_semantics_boundary = value;
    }

    /// Returns whether pointer-related user actions are blocked.
    #[inline]
    pub fn blocks_user_actions(&self) -> bool {
        self.blocks_user_actions
    }

    /// Sets whether pointer-related user actions from this configuration and
    /// its subtree are blocked.
    ///
    /// On merge, the policy filters only this configuration's contribution;
    /// it does not remove actions registered by the receiving ancestor.
    pub fn set_blocks_user_actions(&mut self, value: bool) {
        self.blocks_user_actions = value;
    }

    /// Returns whether contributing children are requested to form explicit
    /// semantics nodes.
    #[inline]
    pub fn explicit_child_nodes(&self) -> bool {
        self.explicit_child_nodes
    }

    /// Requests that contributing children form explicit semantics nodes.
    pub fn set_explicit_child_nodes(&mut self, value: bool) {
        self.explicit_child_nodes = value;
    }

    /// Returns whether the entire descendant subtree merges into this
    /// node's semantics node.
    #[inline]
    pub fn is_merging_semantics_of_descendants(&self) -> bool {
        self.descendant_semantics_merge == DescendantSemanticsMerge::MergeIntoThisNode
    }

    /// Sets whether the entire descendant subtree merges into this node's
    /// semantics node (`RenderMergeSemantics` parity — see the field doc).
    pub fn set_merging_semantics_of_descendants(&mut self, value: bool) {
        self.descendant_semantics_merge = if value {
            DescendantSemanticsMerge::MergeIntoThisNode
        } else {
            DescendantSemanticsMerge::SeparateNodes
        };
        self.mark_annotated();
    }

    // ========================================================================
    // Role
    // ========================================================================

    /// Sets `role` while `value` holds, and clears it when `value` is false
    /// and `role` is the node's current role.
    fn set_role_while(&mut self, role: SemanticsRole, value: bool) {
        if value {
            self.attributes.role = role;
        } else if self.attributes.role == role {
            self.attributes.role = SemanticsRole::None;
        }
        self.mark_annotated();
    }

    /// Makes this node a button, or clears that role.
    pub fn set_button(&mut self, value: bool) {
        self.set_role_while(SemanticsRole::Button, value);
    }

    /// Returns whether this node is a button.
    #[inline]
    pub fn is_button(&self) -> bool {
        self.attributes.role == SemanticsRole::Button
    }

    /// Makes this node a link, or clears that role.
    pub fn set_link(&mut self, value: bool) {
        self.set_role_while(SemanticsRole::Link, value);
    }

    /// Returns whether this node is a link.
    #[inline]
    pub fn is_link(&self) -> bool {
        self.attributes.role == SemanticsRole::Link
    }

    /// Makes this node a text field, or clears that role. A node that is
    /// already a text field of another kind (password, multi-line) keeps it.
    pub fn set_text_field(&mut self, value: bool) {
        let is_text_input = self.attributes.role.is_text_input();
        if value && !is_text_input {
            self.attributes.role = SemanticsRole::TextInput;
        } else if !value && is_text_input {
            self.attributes.role = SemanticsRole::None;
        }
        self.mark_annotated();
    }

    /// Returns whether this node is a text field of any kind.
    #[inline]
    pub fn is_text_field(&self) -> bool {
        self.attributes.role.is_text_input()
    }

    /// Makes this node a password field, whose characters are obscured; false
    /// turns a password field back into a plain text field.
    pub fn set_obscured(&mut self, value: bool) {
        if value {
            self.attributes.role = SemanticsRole::PasswordInput;
        } else if self.attributes.role == SemanticsRole::PasswordInput {
            self.attributes.role = SemanticsRole::TextInput;
        }
        self.mark_annotated();
    }

    /// Returns whether this node is a password field.
    #[inline]
    pub fn is_obscured(&self) -> bool {
        self.attributes.role == SemanticsRole::PasswordInput
    }

    /// Makes this node a multi-line text field; false turns one back into a
    /// single-line field.
    pub fn set_multiline(&mut self, value: bool) {
        if value {
            self.attributes.role = SemanticsRole::MultilineTextInput;
        } else if self.attributes.role == SemanticsRole::MultilineTextInput {
            self.attributes.role = SemanticsRole::TextInput;
        }
        self.mark_annotated();
    }

    /// Returns whether this node is a multi-line text field.
    #[inline]
    pub fn is_multiline(&self) -> bool {
        self.attributes.role == SemanticsRole::MultilineTextInput
    }

    /// Makes this node a slider, or clears that role.
    pub fn set_slider(&mut self, value: bool) {
        self.set_role_while(SemanticsRole::Slider, value);
    }

    /// Returns whether this node is a slider.
    #[inline]
    pub fn is_slider(&self) -> bool {
        self.attributes.role == SemanticsRole::Slider
    }

    /// Makes this node an image, or clears that role.
    pub fn set_image(&mut self, value: bool) {
        self.set_role_while(SemanticsRole::Image, value);
    }

    /// Returns whether this node is an image.
    #[inline]
    pub fn is_image(&self) -> bool {
        self.attributes.role == SemanticsRole::Image
    }

    /// Makes this node a heading, or clears that role and its level.
    ///
    /// A heading without a [level](Self::set_heading_level) is announced as a
    /// heading of no particular level.
    pub fn set_header(&mut self, value: bool) {
        self.set_role_while(SemanticsRole::Heading, value);
        if !value {
            self.attributes.heading_level = None;
        }
    }

    /// Returns whether this node is a heading.
    #[inline]
    pub fn is_header(&self) -> bool {
        self.attributes.role == SemanticsRole::Heading
    }

    /// Makes this node a heading at `level` of the outline.
    pub fn set_heading_level(&mut self, level: HeadingLevel) {
        self.attributes.role = SemanticsRole::Heading;
        self.attributes.heading_level = Some(level);
        self.mark_annotated();
    }

    /// Returns the outline level of this heading, when one was given.
    #[inline]
    #[must_use]
    pub fn heading_level(&self) -> Option<HeadingLevel> {
        self.attributes.heading_level
    }

    // ========================================================================
    // States
    // ========================================================================

    /// The states this configuration declares.
    #[inline]
    #[must_use]
    pub fn states(&self) -> &States {
        &self.attributes.states
    }

    fn declare(&mut self, update: impl FnOnce(&mut States)) {
        update(&mut self.attributes.states);
        self.mark_annotated();
    }

    /// Sets the checked state of a checkable role, or the pressed state of a
    /// button; `None` declares none.
    pub fn set_toggled(&mut self, toggled: Option<Toggled>) {
        self.declare(|states| states.toggled = toggled);
    }

    /// Returns the checked or pressed state, when declared.
    #[inline]
    #[must_use]
    pub fn toggled(&self) -> Option<Toggled> {
        self.attributes.states.toggled
    }

    /// Declares whether this node is selected.
    pub fn set_selected(&mut self, value: bool) {
        self.declare(|states| states.selected = Some(value));
    }

    /// Returns whether this node is selected.
    #[inline]
    pub fn is_selected(&self) -> bool {
        self.attributes.states.selected == Some(true)
    }

    /// Declares whether this expandable node is expanded.
    pub fn set_expanded(&mut self, value: bool) {
        self.declare(|states| states.expanded = Some(value));
    }

    /// Returns the expanded state, when this node is expandable.
    #[inline]
    #[must_use]
    pub fn expanded(&self) -> Option<bool> {
        self.attributes.states.expanded
    }

    /// Returns whether this node is expanded.
    #[inline]
    pub fn is_expanded(&self) -> bool {
        self.attributes.states.expanded == Some(true)
    }

    /// Declares whether this node accepts interaction; `None` declares
    /// nothing.
    pub fn set_enabled(&mut self, enabled: Option<bool>) {
        self.declare(|states| states.enabled = enabled);
    }

    /// Returns whether this node accepts interaction, when declared.
    #[inline]
    pub fn is_enabled(&self) -> Option<bool> {
        self.attributes.states.enabled
    }

    /// Declares whether this node's value is read-only.
    pub fn set_read_only(&mut self, value: bool) {
        self.declare(|states| states.read_only = Some(value));
    }

    /// Returns whether this node's value is read-only.
    #[inline]
    pub fn is_read_only(&self) -> bool {
        self.attributes.states.read_only == Some(true)
    }

    /// Declares whether this node can take the keyboard focus.
    pub fn set_focusable(&mut self, value: bool) {
        self.declare(|states| states.focusable = Some(value));
    }

    /// Returns whether this node can take the keyboard focus.
    #[inline]
    pub fn is_focusable(&self) -> bool {
        self.attributes.states.focusable == Some(true)
    }

    /// Declares whether this node has the keyboard focus.
    pub fn set_focused(&mut self, value: bool) {
        self.declare(|states| states.focused = Some(value));
    }

    /// Returns whether this node has the keyboard focus.
    #[inline]
    pub fn is_focused(&self) -> bool {
        self.attributes.states.focused == Some(true)
    }

    /// Declares whether this node is hidden from assistive technology.
    pub fn set_hidden(&mut self, value: bool) {
        self.declare(|states| states.hidden = Some(value));
    }

    /// Returns whether this node is hidden.
    #[inline]
    pub fn is_hidden(&self) -> bool {
        self.attributes.states.hidden == Some(true)
    }

    /// Declares whether this node is modal: while it is shown, assistive
    /// technology reaches only it and its descendants. A dialog is.
    pub fn set_modal(&mut self, value: bool) {
        self.declare(|states| states.modal = Some(value));
    }

    /// Returns whether this node is modal.
    #[inline]
    pub fn is_modal(&self) -> bool {
        self.attributes.states.modal == Some(true)
    }

    /// Declares whether this form control must be filled before submitting.
    pub fn set_required(&mut self, value: bool) {
        self.declare(|states| states.required = Some(value));
    }

    /// Returns whether this form control must be filled.
    #[inline]
    pub fn is_required(&self) -> bool {
        self.attributes.states.required == Some(true)
    }

    /// Declares whether this form control's value failed validation.
    pub fn set_invalid(&mut self, value: bool) {
        self.declare(|states| states.invalid = Some(value));
    }

    /// Returns whether this form control's value failed validation.
    #[inline]
    pub fn is_invalid(&self) -> bool {
        self.attributes.states.invalid == Some(true)
    }

    /// Declares whether this node scopes a route. Read by the framework,
    /// not published.
    pub fn set_scopes_route(&mut self, value: bool) {
        self.declare(|states| states.scopes_route = Some(value));
    }

    /// Returns whether this node scopes a route.
    #[inline]
    pub fn scopes_route(&self) -> bool {
        self.attributes.states.scopes_route == Some(true)
    }

    /// Declares whether this node names a route. Read by the framework, not
    /// published.
    pub fn set_names_route(&mut self, value: bool) {
        self.declare(|states| states.names_route = Some(value));
    }

    /// Returns whether this node names a route.
    #[inline]
    pub fn names_route(&self) -> bool {
        self.attributes.states.names_route == Some(true)
    }

    /// Makes this node a live region, whose changes assistive technology
    /// announces without the user moving to it: politely, after what is being
    /// read, or assertively, interrupting it. `None` makes it an ordinary node.
    pub fn set_live_region(&mut self, politeness: Option<Assertiveness>) {
        self.attributes.live = politeness;
        self.mark_annotated();
    }

    /// How this live region's changes are announced, or `None` when it is
    /// not one.
    #[must_use]
    pub fn live_region(&self) -> Option<Assertiveness> {
        self.attributes.live
    }

    // ========================================================================
    // Label, Value, Hint
    // ========================================================================

    /// Sets the label.
    pub fn set_label(&mut self, label: impl Into<AttributedString>) {
        self.attributes.label = Some(label.into());
        self.mark_annotated();
    }

    /// Returns the label.
    #[inline]
    pub fn label(&self) -> Option<&AttributedString> {
        self.attributes.label.as_ref()
    }

    /// Sets the value.
    pub fn set_value(&mut self, value: impl Into<AttributedString>) {
        self.attributes.value = Some(value.into());
        self.mark_annotated();
    }

    /// Returns the value.
    #[inline]
    pub fn value(&self) -> Option<&AttributedString> {
        self.attributes.value.as_ref()
    }

    /// Publishes a validated numeric range. A node without a role becomes a
    /// slider; one with a role (a progress bar, a spin button, a scroll
    /// adjuster) keeps it.
    pub fn set_numeric_range(&mut self, range: crate::NumericRange) {
        self.attributes.numeric_range = Some(range);
        if self.attributes.role == SemanticsRole::None {
            self.attributes.role = SemanticsRole::Slider;
        }
        self.mark_annotated();
    }

    /// The validated range currently described by this node.
    #[must_use]
    pub const fn numeric_range(&self) -> Option<crate::NumericRange> {
        self.attributes.numeric_range
    }

    /// Sets the increased value.
    pub fn set_increased_value(&mut self, value: impl Into<AttributedString>) {
        self.attributes.increased_value = Some(value.into());
        self.mark_annotated();
    }

    /// Returns the increased value.
    #[inline]
    pub fn increased_value(&self) -> Option<&AttributedString> {
        self.attributes.increased_value.as_ref()
    }

    /// Sets the decreased value.
    pub fn set_decreased_value(&mut self, value: impl Into<AttributedString>) {
        self.attributes.decreased_value = Some(value.into());
        self.mark_annotated();
    }

    /// Returns the decreased value.
    #[inline]
    pub fn decreased_value(&self) -> Option<&AttributedString> {
        self.attributes.decreased_value.as_ref()
    }

    /// Sets the hint.
    pub fn set_hint(&mut self, hint: impl Into<AttributedString>) {
        self.attributes.hint = Some(hint.into());
        self.mark_annotated();
    }

    /// Returns the hint.
    #[inline]
    pub fn hint(&self) -> Option<&AttributedString> {
        self.attributes.hint.as_ref()
    }

    /// Sets the tooltip.
    pub fn set_tooltip(&mut self, tooltip: impl Into<SmolStr>) {
        self.attributes.tooltip = Some(tooltip.into());
        self.mark_annotated();
    }

    /// Returns the tooltip.
    #[inline]
    pub fn tooltip(&self) -> Option<&str> {
        self.attributes.tooltip.as_deref()
    }

    /// Sets what an empty text field shows in place of a value, which
    /// assistive technology reads while the field is empty.
    pub fn set_placeholder(&mut self, placeholder: impl Into<SmolStr>) {
        self.attributes.placeholder = Some(placeholder.into());
        self.mark_annotated();
    }

    /// Returns the placeholder.
    #[inline]
    pub fn placeholder(&self) -> Option<&str> {
        self.attributes.placeholder.as_deref()
    }

    // ========================================================================
    // Text Direction
    // ========================================================================

    /// Sets the text direction.
    pub fn set_text_direction(&mut self, direction: TextDirection) {
        self.attributes.text_direction = Some(direction);
        self.mark_annotated();
    }

    /// Returns the text direction.
    #[inline]
    pub fn text_direction(&self) -> Option<TextDirection> {
        self.attributes.text_direction
    }

    // ========================================================================
    // Actions
    // ========================================================================

    /// Adds an action handler.
    pub fn add_action(&mut self, action: SemanticsAction, handler: SemanticsActionHandler) {
        self.actions.insert(action, handler);
        self.mark_annotated();
    }

    /// Removes an action.
    pub fn remove_action(&mut self, action: SemanticsAction) {
        self.actions.remove(&action);
        self.mark_annotated();
    }

    /// Returns whether an action is available.
    #[inline]
    pub fn has_action(&self, action: SemanticsAction) -> bool {
        self.actions.contains_key(&action)
    }

    /// Returns the registered action handler for a given action.
    ///
    /// This intentionally exposes registration state for tree assembly. Code
    /// dispatching an action received from a platform must also verify that the
    /// action's bit is present in [`Self::effective_actions_as_bits`].
    pub fn action_handler(&self, action: SemanticsAction) -> Option<&SemanticsActionHandler> {
        self.actions.get(&action)
    }

    /// Record the ancestor reveal route discovered during render assembly.
    /// This structural directive does not annotate otherwise transparent nodes.
    pub fn set_has_reveal_ancestor(&mut self, value: bool) {
        self.has_reveal_ancestor = value;
    }

    /// Returns a bitmask of registered actions before blocking policy.
    pub fn actions_as_bits(&self) -> u64 {
        self.actions
            .keys()
            .fold(0u64, |acc, action| acc | action.value())
    }

    /// Returns the action bits that may be exposed to assistive technology.
    ///
    /// When user actions are blocked, pointer and editing actions are removed;
    /// only accessibility-focus lifecycle actions remain. Platform snapshots,
    /// legacy node exports, and future action routing must all use this policy
    /// rather than [`Self::actions_as_bits`].
    pub fn effective_actions_as_bits(&self) -> u64 {
        let actions = self.actions_as_bits()
            | if self.has_reveal_ancestor {
                SemanticsAction::ShowOnScreen.value()
            } else {
                0
            };
        if self.blocks_user_actions {
            actions & UNBLOCKED_USER_ACTIONS_MASK
        } else {
            actions
        }
    }

    // ========================================================================
    // Custom Actions
    // ========================================================================

    /// Adds a custom action.
    pub fn add_custom_action(&mut self, action: CustomSemanticsAction) {
        self.attributes.custom_actions.push(action);
        self.mark_annotated();
    }

    /// Returns the custom actions.
    #[inline]
    pub fn custom_actions(&self) -> &[CustomSemanticsAction] {
        &self.attributes.custom_actions
    }

    /// Returns custom action metadata backed by an effective handler.
    ///
    /// Metadata is construction data, not proof that an operation can be
    /// dispatched. A blocked or unregistered `CustomAction` bit therefore
    /// exposes no custom actions to platform adapters.
    #[inline]
    pub fn effective_custom_actions(&self) -> &[CustomSemanticsAction] {
        if (self.effective_actions_as_bits() & SemanticsAction::CustomAction.value()) != 0 {
            &self.attributes.custom_actions
        } else {
            &[]
        }
    }

    // ========================================================================
    // Tags
    // ========================================================================

    /// Adds a tag.
    pub fn add_tag(&mut self, tag: SemanticsTag) {
        if !self.attributes.tags.contains(&tag) {
            self.attributes.tags.push(tag);
        }
    }

    /// Returns the tags.
    #[inline]
    pub fn tags(&self) -> &[SemanticsTag] {
        &self.attributes.tags
    }

    /// Returns whether a tag is present.
    pub fn has_tag(&self, tag: &SemanticsTag) -> bool {
        self.attributes.tags.iter().any(|t| t == tag)
    }

    // ========================================================================
    // Sort Key
    // ========================================================================

    /// Sets the sort key.
    pub fn set_sort_key(&mut self, key: SemanticsSortKey) {
        self.attributes.sort_key = Some(key);
        self.mark_annotated();
    }

    /// Returns the sort key.
    #[inline]
    pub fn sort_key(&self) -> Option<&SemanticsSortKey> {
        self.attributes.sort_key.as_ref()
    }

    // ========================================================================
    // Hint Overrides
    // ========================================================================

    /// Sets hint overrides.
    pub fn set_hint_overrides(&mut self, overrides: SemanticsHintOverrides) {
        self.attributes.hint_overrides = Some(overrides);
        self.mark_annotated();
    }

    /// Returns hint overrides.
    #[inline]
    pub fn hint_overrides(&self) -> Option<&SemanticsHintOverrides> {
        self.attributes.hint_overrides.as_ref()
    }

    // ========================================================================
    // Scroll Properties
    // ========================================================================

    /// Sets the scroll position.
    pub fn set_scroll_position(&mut self, position: f64) {
        self.attributes.scroll_position = Some(position);
        self.mark_annotated();
    }

    /// Returns the scroll position.
    #[inline]
    pub fn scroll_position(&self) -> Option<f64> {
        self.attributes.scroll_position
    }

    /// Sets the axis of this node's scroll position and extents.
    pub fn set_scroll_axis(&mut self, axis: Axis) {
        self.attributes.scroll_axis = Some(axis);
        self.mark_annotated();
    }

    /// Returns the explicit scroll axis. Scalar ranges without it are vertical.
    #[inline]
    #[must_use]
    pub fn scroll_axis(&self) -> Option<Axis> {
        self.attributes.scroll_axis
    }

    /// Sets the scroll extent maximum.
    pub fn set_scroll_extent_max(&mut self, max: f64) {
        self.attributes.scroll_extent_max = Some(max);
        self.mark_annotated();
    }

    /// Returns the scroll extent maximum.
    #[inline]
    pub fn scroll_extent_max(&self) -> Option<f64> {
        self.attributes.scroll_extent_max
    }

    /// Sets the scroll extent minimum.
    pub fn set_scroll_extent_min(&mut self, min: f64) {
        self.attributes.scroll_extent_min = Some(min);
        self.mark_annotated();
    }

    /// Returns the scroll extent minimum.
    #[inline]
    pub fn scroll_extent_min(&self) -> Option<f64> {
        self.attributes.scroll_extent_min
    }

    /// Sets the scroll index.
    pub fn set_scroll_index(&mut self, index: i32) {
        self.attributes.scroll_index = Some(index);
        self.mark_annotated();
    }

    /// Returns the scroll index.
    #[inline]
    pub fn scroll_index(&self) -> Option<i32> {
        self.attributes.scroll_index
    }

    /// Sets the scroll child count.
    pub fn set_scroll_child_count(&mut self, count: i32) {
        self.attributes.scroll_child_count = Some(count);
        self.mark_annotated();
    }

    /// Returns the scroll child count.
    #[inline]
    pub fn scroll_child_count(&self) -> Option<i32> {
        self.attributes.scroll_child_count
    }

    // ========================================================================
    // Index
    // ========================================================================

    /// Sets the index in parent.
    pub fn set_index_in_parent(&mut self, index: i32) {
        self.attributes.index_in_parent = Some(index);
        self.mark_annotated();
    }

    /// Returns the index in parent.
    #[inline]
    pub fn index_in_parent(&self) -> Option<i32> {
        self.attributes.index_in_parent
    }

    // ========================================================================
    // Platform View
    // ========================================================================

    /// Sets the platform view ID.
    pub fn set_platform_view_id(&mut self, id: i32) {
        self.attributes.platform_view_id = Some(id);
        self.mark_annotated();
    }

    /// Returns the platform view ID.
    #[inline]
    pub fn platform_view_id(&self) -> Option<i32> {
        self.attributes.platform_view_id
    }

    // ========================================================================
    // Text Field Properties
    // ========================================================================

    /// Sets the maximum value length.
    pub fn set_max_value_length(&mut self, length: i32) {
        self.attributes.max_value_length = Some(length);
        self.mark_annotated();
    }

    /// Returns the maximum value length.
    #[inline]
    pub fn max_value_length(&self) -> Option<i32> {
        self.attributes.max_value_length
    }

    /// Sets the current value length.
    pub fn set_current_value_length(&mut self, length: i32) {
        self.attributes.current_value_length = Some(length);
        self.mark_annotated();
    }

    /// Returns the current value length.
    #[inline]
    pub fn current_value_length(&self) -> Option<i32> {
        self.attributes.current_value_length
    }

    // ========================================================================
    // Relations
    // ========================================================================

    /// Declares that the node this configuration forms or merges into is
    /// named by `anchor`, so other nodes' relations to it resolve here.
    pub fn add_anchor(&mut self, anchor: SemanticsAnchor) {
        if !self.attributes.anchors.contains(&anchor) {
            self.attributes.anchors.push(anchor);
        }
        self.mark_annotated();
    }

    /// The anchors naming this node.
    #[inline]
    pub fn anchors(&self) -> &[SemanticsAnchor] {
        &self.attributes.anchors
    }

    /// Relates this node to every node `anchor` names.
    pub fn add_relation(&mut self, relation: SemanticsRelation, anchor: SemanticsAnchor) {
        let entry = (relation, anchor);
        if !self.attributes.relations.contains(&entry) {
            self.attributes.relations.push(entry);
        }
        self.mark_annotated();
    }

    /// This node's relations, each to the nodes its anchor names.
    #[inline]
    pub fn relations(&self) -> &[(SemanticsRelation, SemanticsAnchor)] {
        &self.attributes.relations
    }

    // ========================================================================
    // Role
    // ========================================================================

    /// Sets the [`SemanticsRole`] for this node.
    ///
    /// `SemanticsRole::None` is the default; pass any other variant when
    /// the node is a structural element (Button, TextField, Table, etc.)
    /// the platform adapter should expose with a specific accessibility
    /// role.
    #[inline]
    pub fn set_role(&mut self, role: SemanticsRole) {
        self.attributes.role = role;
        self.mark_annotated();
    }

    /// Returns the [`SemanticsRole`] for this node. Defaults to
    /// `SemanticsRole::None`.
    #[inline]
    pub fn role(&self) -> SemanticsRole {
        self.attributes.role
    }

    /// Builder-style role setter for chained construction.
    #[must_use]
    pub fn with_role(mut self, role: SemanticsRole) -> Self {
        self.set_role(role);
        self
    }

    // ========================================================================
    // Merging and Copying
    // ========================================================================

    /// Returns whether this configuration can share one semantics node with
    /// `other` without dropping modeled information (ADR-0202, decision 4).
    ///
    /// The predicate is symmetric. Labels and hints concatenate, relations and
    /// disjoint actions combine, and first-wins metadata stays compatible. Two
    /// roles, a state both declare, a live region on both sides, competing
    /// non-empty values, numeric ranges, platform views or text lengths, or a
    /// repeated action require separate nodes. A state declared `false` counts:
    /// a node's shape does not depend on its current values.
    #[must_use]
    pub fn is_compatible_with(&self, other: &SemanticsConfiguration) -> bool {
        if !self.has_been_annotated || !other.has_been_annotated {
            return true;
        }
        if self.actions_as_bits() & other.actions_as_bits() != 0
            || self.has_explicit_role() && other.has_explicit_role()
        {
            return false;
        }

        // Exhaustive: a new attribute does not compile until it decides here.
        let SemanticsAttributes {
            states,
            live,
            label: _,
            value,
            numeric_range,
            increased_value: _,
            decreased_value: _,
            hint: _,
            tooltip: _,
            placeholder: _,
            text_direction: _,
            custom_actions: _,
            tags: _,
            sort_key: _,
            hint_overrides: _,
            scroll_position,
            scroll_axis,
            scroll_extent_max,
            scroll_extent_min,
            index_in_parent,
            scroll_index,
            scroll_child_count,
            platform_view_id,
            max_value_length,
            current_value_length,
            heading_level: _,
            anchors: _,
            relations: _,
            role: _,
        } = &self.attributes;
        let theirs = &other.attributes;
        let both = |a: bool, b: bool| a && b;
        let non_empty = |value: &Option<AttributedString>| {
            value.as_ref().is_some_and(|v| !v.as_str().is_empty())
        };

        // A scroll group is one scrollable's metrics: two of them never combine.
        let scrolls = scroll_position.is_some()
            || scroll_axis.is_some()
            || scroll_extent_max.is_some()
            || scroll_extent_min.is_some()
            || scroll_index.is_some();
        let their_scrolls = theirs.scroll_position.is_some()
            || theirs.scroll_axis.is_some()
            || theirs.scroll_extent_max.is_some()
            || theirs.scroll_extent_min.is_some()
            || theirs.scroll_index.is_some();

        !(states.conflicts(&theirs.states)
            || both(scrolls, their_scrolls)
            // A collection group is one entry's place in one set.
            || both(
                index_in_parent.is_some() || scroll_child_count.is_some(),
                theirs.index_in_parent.is_some() || theirs.scroll_child_count.is_some(),
            )
            || both(live.is_some(), theirs.live.is_some())
            || both(non_empty(value), non_empty(&theirs.value))
            || both(numeric_range.is_some(), theirs.numeric_range.is_some())
            || both(
                platform_view_id.is_some(),
                theirs.platform_view_id.is_some(),
            )
            || both(
                max_value_length.is_some(),
                theirs.max_value_length.is_some(),
            )
            || both(
                current_value_length.is_some(),
                theirs.current_value_length.is_some(),
            ))
    }

    /// A role, or the route scope a page declares, which a second one
    /// cannot share a node with.
    fn has_explicit_role(&self) -> bool {
        self.attributes.role != SemanticsRole::None || self.scopes_route()
    }

    /// Absorbs the semantic information from another configuration.
    ///
    /// The merge rules, per group (ADR-0202, decision 4):
    ///
    /// - **States** — each by its own rule ([`States`]); a `true` wins over
    ///   a `false`, and `Mixed` over `On` over `Off`.
    /// - **Role** — by specificity, see `more_specific_role`; without a
    ///   role on either side, none.
    /// - **Actions** — absorb every action whose handler the child
    ///   defined. If `other.blocks_user_actions == true`, only actions in
    ///   the `UNBLOCKED_USER_ACTIONS_MASK` mask cross the boundary; the rest are
    ///   filtered out.
    /// - **Custom actions** — concatenate only metadata backed by an effective
    ///   `CustomAction` handler at each source.
    /// - **Tags, anchors, relations** — union.
    /// - **Label / hint** — concatenate, text-direction aware.
    /// - **Everything else** — first wins: the receiver keeps what it has.
    ///
    /// `blocks_user_actions` on the *parent* is unchanged by absorb —
    /// only the child's flag controls the action-mask filter applied
    /// to the child's actions during the merge.
    pub fn absorb(&mut self, other: &SemanticsConfiguration) {
        if !other.has_been_annotated {
            return;
        }

        // Metadata already present on the receiver belongs to the receiver's
        // source. Freeze that source's availability before child handlers are
        // merged, otherwise a child's CustomAction bit could make unrelated
        // receiver metadata appear routable.
        if self.effective_custom_actions().is_empty() {
            self.attributes.custom_actions.clear();
        }

        // ----- actions (blocked / unblocked filter) -----
        let effective_other_actions = other.effective_actions_as_bits();
        for (action, handler) in &other.actions {
            if (action.value() & effective_other_actions) != 0 {
                self.actions.insert(*action, Arc::clone(handler));
            }
        }
        let other_custom_actions = other.effective_custom_actions().to_vec();

        // Exhaustive: a new attribute does not compile until it decides here.
        let SemanticsAttributes {
            states,
            live,
            label,
            value,
            numeric_range,
            increased_value,
            decreased_value,
            hint,
            tooltip,
            placeholder,
            text_direction,
            custom_actions: _,
            tags,
            sort_key,
            hint_overrides,
            scroll_position,
            scroll_axis,
            scroll_extent_max,
            scroll_extent_min,
            index_in_parent,
            scroll_index,
            scroll_child_count,
            platform_view_id,
            max_value_length,
            current_value_length,
            heading_level,
            anchors,
            relations,
            role,
        } = &other.attributes;
        let mine = &mut self.attributes;

        mine.states.merge(states);
        mine.role = more_specific_role(mine.role, *role);
        mine.custom_actions.extend(other_custom_actions);
        for tag in tags {
            if !mine.tags.contains(tag) {
                mine.tags.push(tag.clone());
            }
        }
        for anchor in anchors {
            if !mine.anchors.contains(anchor) {
                mine.anchors.push(anchor.clone());
            }
        }
        for entry in relations {
            if !mine.relations.contains(entry) {
                mine.relations.push(entry.clone());
            }
        }

        // ----- label and hint (concatenate, text-direction aware) -----
        let mine_dir = mine.text_direction.unwrap_or(TextDirection::Ltr);
        let theirs_dir = text_direction.unwrap_or(TextDirection::Ltr);
        let concat =
            |target: &mut Option<AttributedString>, source: &Option<AttributedString>| match (
                target.as_ref(),
                source,
            ) {
                (_, None) => {}
                (None, Some(source)) => *target = Some(source.clone()),
                (Some(existing), Some(source)) => {
                    *target = Some(concat_attributed_string(
                        existing, mine_dir, source, theirs_dir,
                    ));
                }
            };
        concat(&mut mine.label, label);
        concat(&mut mine.hint, hint);

        // ----- first wins -----
        fn keep<T: Clone>(target: &mut Option<T>, source: Option<&T>) {
            if target.is_none() {
                *target = source.cloned();
            }
        }
        // A scroll group is one scrollable's metrics: the receiver keeps its
        // own whole, or takes the contributor's whole, never a mix of the two.
        if mine.scroll_axis.is_none()
            && mine.scroll_position.is_none()
            && mine.scroll_extent_max.is_none()
            && mine.scroll_extent_min.is_none()
            && mine.scroll_index.is_none()
        {
            mine.scroll_axis = *scroll_axis;
            mine.scroll_position = *scroll_position;
            mine.scroll_extent_max = *scroll_extent_max;
            mine.scroll_extent_min = *scroll_extent_min;
            mine.scroll_index = *scroll_index;
        }
        keep(&mut mine.live, live.as_ref());
        keep(&mut mine.numeric_range, numeric_range.as_ref());
        keep(&mut mine.value, value.as_ref());
        keep(&mut mine.increased_value, increased_value.as_ref());
        keep(&mut mine.decreased_value, decreased_value.as_ref());
        keep(&mut mine.tooltip, tooltip.as_ref());
        keep(&mut mine.placeholder, placeholder.as_ref());
        keep(&mut mine.sort_key, sort_key.as_ref());
        keep(&mut mine.text_direction, text_direction.as_ref());
        keep(&mut mine.hint_overrides, hint_overrides.as_ref());
        // A collection group is copied whole, never mixed from two entries.
        if mine.index_in_parent.is_none() && mine.scroll_child_count.is_none() {
            mine.index_in_parent = *index_in_parent;
            mine.scroll_child_count = *scroll_child_count;
        }
        keep(&mut mine.platform_view_id, platform_view_id.as_ref());
        keep(&mut mine.max_value_length, max_value_length.as_ref());
        keep(
            &mut mine.current_value_length,
            current_value_length.as_ref(),
        );
        keep(&mut mine.heading_level, heading_level.as_ref());

        self.has_been_annotated |= other.has_been_annotated;
    }
}

/// The role one node built from two takes (ADR-0202, decision 4).
///
/// A structural role (tab, list item, dialog, …) is authored structure and
/// outranks every interactive kind, as an explicit role always did. Among
/// interactive kinds the more specific wins: a checkable role, then a button,
/// then a link, text field or slider, then an image or heading. On a tie the
/// receiver keeps its role. The order is what makes a merged list tile with a
/// radio a radio button rather than a button
/// (`merge_semantics_over_a_tile_and_radio_announces_as_one_radio_button`).
pub(crate) fn more_specific_role(
    receiver: SemanticsRole,
    contributor: SemanticsRole,
) -> SemanticsRole {
    fn rank(role: SemanticsRole) -> u8 {
        match role {
            SemanticsRole::None => 0,
            SemanticsRole::Image | SemanticsRole::Heading => 1,
            SemanticsRole::Link
            | SemanticsRole::Slider
            | SemanticsRole::TextInput
            | SemanticsRole::MultilineTextInput
            | SemanticsRole::PasswordInput => 2,
            SemanticsRole::Button => 3,
            SemanticsRole::CheckBox | SemanticsRole::RadioButton | SemanticsRole::Switch => 4,
            _ => 5,
        }
    }
    if rank(contributor) > rank(receiver) {
        contributor
    } else {
        receiver
    }
}

impl std::fmt::Debug for SemanticsConfiguration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticsConfiguration")
            .field("is_semantics_boundary", &self.is_semantics_boundary)
            .field(
                "is_merging_semantics_of_descendants",
                &self.is_merging_semantics_of_descendants(),
            )
            .field("role", &self.attributes.role)
            .field("states", &self.attributes.states)
            .field("label", &self.attributes.label)
            .field("value", &self.attributes.value)
            .field("hint", &self.attributes.hint)
            .field("actions", &self.actions.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_compatibility_is_symmetric(
        left: &SemanticsConfiguration,
        right: &SemanticsConfiguration,
        expected: bool,
    ) {
        assert_eq!(left.is_compatible_with(right), expected);
        assert_eq!(right.is_compatible_with(left), expected);
    }

    #[test]
    fn compatibility_rules_for_modeled_fields() {
        let empty = SemanticsConfiguration::new();
        let mut label_a = SemanticsConfiguration::new();
        label_a.set_label("Alpha");
        let mut label_b = SemanticsConfiguration::new();
        label_b.set_label("Beta");
        assert_compatibility_is_symmetric(&empty, &label_a, true);
        assert_compatibility_is_symmetric(&label_a, &label_b, true);

        let mut tap = SemanticsConfiguration::new();
        tap.add_action(SemanticsAction::Tap, Arc::new(|_, _| {}));
        let mut long_press = SemanticsConfiguration::new();
        long_press.add_action(SemanticsAction::LongPress, Arc::new(|_, _| {}));
        let mut another_tap = SemanticsConfiguration::new();
        another_tap.add_action(SemanticsAction::Tap, Arc::new(|_, _| {}));
        assert_compatibility_is_symmetric(&tap, &long_press, true);
        assert_compatibility_is_symmetric(&tap, &another_tap, false);

        let mut selected_a = SemanticsConfiguration::new();
        selected_a.set_selected(true);
        let mut selected_b = SemanticsConfiguration::new();
        selected_b.set_selected(true);
        assert_compatibility_is_symmetric(&selected_a, &selected_b, false);

        // Two scrollables never share one node, whichever metrics each declares.
        let mut scroll_a = SemanticsConfiguration::new();
        scroll_a.set_scroll_position(10.0);
        let mut scroll_b = SemanticsConfiguration::new();
        scroll_b.set_scroll_extent_max(100.0);
        assert_compatibility_is_symmetric(&scroll_a, &scroll_b, false);
        assert_compatibility_is_symmetric(&scroll_a, &label_a, true);
        let mut index_a = SemanticsConfiguration::new();
        index_a.set_scroll_index(2);
        let mut index_b = SemanticsConfiguration::new();
        index_b.set_scroll_index(5);
        assert_compatibility_is_symmetric(&index_a, &index_b, false);
        let mut entry_a = SemanticsConfiguration::new();
        entry_a.set_index_in_parent(0);
        let mut entry_b = SemanticsConfiguration::new();
        entry_b.set_index_in_parent(1);
        assert_compatibility_is_symmetric(&entry_a, &entry_b, false);

        let mut value_a = SemanticsConfiguration::new();
        value_a.set_value("first");
        let mut value_b = SemanticsConfiguration::new();
        value_b.set_value("second");
        let mut empty_value = SemanticsConfiguration::new();
        empty_value.set_value("");
        assert_compatibility_is_symmetric(&value_a, &value_b, false);
        assert_compatibility_is_symmetric(&value_a, &empty_value, true);

        for set_competing_field in [
            SemanticsConfiguration::set_platform_view_id as fn(&mut SemanticsConfiguration, i32),
            SemanticsConfiguration::set_max_value_length,
            SemanticsConfiguration::set_current_value_length,
        ] {
            let mut left = SemanticsConfiguration::new();
            set_competing_field(&mut left, 1);
            let mut right = SemanticsConfiguration::new();
            set_competing_field(&mut right, 2);
            assert_compatibility_is_symmetric(&left, &right, false);
        }

        let mut dialog = SemanticsConfiguration::new();
        dialog.set_role(SemanticsRole::Dialog);
        let mut list_item = SemanticsConfiguration::new();
        list_item.set_role(SemanticsRole::ListItem);
        assert_compatibility_is_symmetric(&dialog, &list_item, false);

        // Any second role conflicts, interactive kinds included, and so does a
        // page's route scope.
        for set_role in [
            SemanticsConfiguration::set_button as fn(&mut SemanticsConfiguration, bool),
            SemanticsConfiguration::set_text_field,
            SemanticsConfiguration::set_slider,
            SemanticsConfiguration::set_link,
            SemanticsConfiguration::set_image,
            SemanticsConfiguration::set_header,
            SemanticsConfiguration::set_scopes_route,
        ] {
            let mut second = SemanticsConfiguration::new();
            set_role(&mut second, true);
            assert_compatibility_is_symmetric(&dialog, &second, false);
        }

        // A state declared `false` is declared: the node's shape does not
        // depend on the state's current value.
        let mut unfocused = SemanticsConfiguration::new();
        unfocused.set_focused(false);
        let mut focused = SemanticsConfiguration::new();
        focused.set_focused(true);
        assert_compatibility_is_symmetric(&unfocused, &focused, false);
        let mut unrelated = SemanticsConfiguration::new();
        unrelated.set_selected(false);
        assert_compatibility_is_symmetric(&unfocused, &unrelated, true);

        let mut live_a = SemanticsConfiguration::new();
        live_a.set_live_region(Some(Assertiveness::Polite));
        let mut live_b = SemanticsConfiguration::new();
        live_b.set_live_region(Some(Assertiveness::Assertive));
        assert_compatibility_is_symmetric(&live_a, &live_b, false);

        let mut tooltip_a = SemanticsConfiguration::new();
        tooltip_a.set_tooltip("First");
        let mut tooltip_b = SemanticsConfiguration::new();
        tooltip_b.set_tooltip("Second");
        assert_compatibility_is_symmetric(&tooltip_a, &tooltip_b, true);

        let mut structural_a = SemanticsConfiguration::new();
        structural_a.set_semantics_boundary(true);
        structural_a.set_explicit_child_nodes(true);
        structural_a.set_blocks_user_actions(true);
        let mut structural_b = SemanticsConfiguration::new();
        structural_b.set_semantics_boundary(true);
        structural_b.set_explicit_child_nodes(true);
        structural_b.set_blocks_user_actions(true);
        assert_compatibility_is_symmetric(&structural_a, &structural_b, true);
    }

    /// A forced merge keeps one scrollable's metrics: a receiver with any
    /// scroll metadata keeps its group whole, and a receiver with none takes
    /// the contributor's whole.
    #[test]
    fn a_forced_merge_keeps_one_scroll_group() {
        let mut vertical = SemanticsConfiguration::new();
        vertical.set_scroll_position(10.0);
        let mut horizontal = SemanticsConfiguration::new();
        horizontal.set_scroll_axis(Axis::Horizontal);
        horizontal.set_scroll_extent_max(100.0);

        let mut kept = vertical.clone();
        kept.absorb(&horizontal);
        assert_eq!(
            (
                kept.scroll_axis(),
                kept.scroll_position(),
                kept.scroll_extent_max()
            ),
            (None, Some(10.0), None),
            "the receiver's group is not mixed with the contributor's"
        );

        let mut taken = SemanticsConfiguration::new();
        taken.set_label("receiver");
        taken.absorb(&horizontal);
        assert_eq!(
            (
                taken.scroll_axis(),
                taken.scroll_position(),
                taken.scroll_extent_max()
            ),
            (Some(Axis::Horizontal), None, Some(100.0)),
            "a receiver without one takes the contributor's whole group"
        );
    }

    /// A forced merge takes each state by its type's rule: `Mixed` over `On`
    /// over `Off`, and a declared `true` over a declared `false`.
    #[test]
    fn state_merge_precedence() {
        let merged = |receiver: fn(&mut SemanticsConfiguration),
                      contributor: fn(&mut SemanticsConfiguration)| {
            let mut a = SemanticsConfiguration::new();
            receiver(&mut a);
            let mut b = SemanticsConfiguration::new();
            contributor(&mut b);
            a.absorb(&b);
            a
        };
        let on = merged(
            |c| c.set_toggled(Some(Toggled::Off)),
            |c| c.set_toggled(Some(Toggled::On)),
        );
        assert_eq!(on.toggled(), Some(Toggled::On));
        let mixed = merged(
            |c| c.set_toggled(Some(Toggled::Mixed)),
            |c| c.set_toggled(Some(Toggled::On)),
        );
        assert_eq!(mixed.toggled(), Some(Toggled::Mixed));
        let focused = merged(|c| c.set_focused(false), |c| c.set_focused(true));
        assert!(focused.is_focused());
        let enabled = merged(|c| c.set_enabled(Some(false)), |_| {});
        assert_eq!(enabled.is_enabled(), Some(false));
    }

    /// A forced merge keeps the more specific role: an authored structural
    /// role over any interactive kind, a checkable over a button, a button
    /// over a link or text field, and the receiver's role on a tie.
    #[test]
    fn role_precedence_settles_forced_merges() {
        use SemanticsRole as R;
        let rows: &[(R, R, R)] = &[
            (R::Button, R::RadioButton, R::RadioButton),
            (R::RadioButton, R::Button, R::RadioButton),
            (R::Button, R::Link, R::Button),
            (R::TextInput, R::Button, R::Button),
            (R::Tab, R::Button, R::Tab),
            (R::Heading, R::Link, R::Link),
            (R::None, R::Image, R::Image),
            (R::Link, R::Slider, R::Link),
        ];
        for &(receiver, contributor, expected) in rows {
            assert_eq!(
                more_specific_role(receiver, contributor),
                expected,
                "{receiver} absorbing {contributor}"
            );
        }
    }

    #[test]
    fn blocked_effective_action_mask_keeps_only_accessibility_focus_lifecycle() {
        for &action in SemanticsAction::ALL {
            let mut config = SemanticsConfiguration::new();
            config.add_action(action, Arc::new(|_, _| {}));
            config.set_blocks_user_actions(true);

            let expected = if matches!(
                action,
                SemanticsAction::DidGainAccessibilityFocus
                    | SemanticsAction::DidLoseAccessibilityFocus
            ) {
                action.value()
            } else {
                0
            };
            assert_eq!(
                config.effective_actions_as_bits(),
                expected,
                "unexpected blocked availability for {}",
                action.name(),
            );
        }
    }
}
