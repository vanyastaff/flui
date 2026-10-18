//! The configuration a render object fills to describe itself, and the rules
//! by which two configurations share a node or merge into one (ADR-0206).

use std::sync::Arc;

use flui_foundation::geometry::Axis;

use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use smol_str::SmolStr;

use crate::{
    action::{CustomActionId, SemanticsAction, SemanticsActionHandler},
    attributes::SemanticsAttributes,
    properties::{
        AttributedString, HeadingLevel, SemanticsHintOverrides, SemanticsSortKey, SemanticsTag,
        TextDirection, UNBLOCKED_USER_ACTIONS_MASK, concat_attributed_string,
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

/// One custom action a node declares, as
/// [`SemanticsConfiguration::custom_action`] finds it.
#[derive(Clone, Copy)]
pub struct DeclaredCustomAction<'a> {
    /// The label assistive technology offers.
    pub label: &'a str,
    /// The handler that runs it.
    pub handler: &'a SemanticsActionHandler,
    /// The id the action's own configuration gave it, which its handler
    /// receives even after a merge publishes it under another.
    pub own_id: CustomActionId,
}

impl std::fmt::Debug for DeclaredCustomAction<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeclaredCustomAction")
            .field("label", &self.label)
            .field("own_id", &self.own_id)
            .finish_non_exhaustive()
    }
}

/// Configuration describing the semantic properties of a render object.
///
/// A render object fills one in its `describe_semantics_configuration`;
/// assembly merges it into the nearest node-forming ancestor or forms a node
/// of its own, and publication translates the node's attributes.
///
/// # Example
///
/// ```
/// use std::sync::Arc;
///
/// use flui_semantics::{SemanticsAction, SemanticsConfiguration, SemanticsRole};
///
/// let mut config = SemanticsConfiguration::new();
/// config.set_label("Submit button");
/// config.set_role(SemanticsRole::Button);
/// config.add_action(SemanticsAction::Click, Arc::new(|_| {
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

    /// Drops preceding sibling contributions up to the nearest formed boundary.
    blocks_previous_siblings: bool,
    collection: Option<(crate::SemanticsCollection, crate::SemanticsCollectionOrigin)>,
    index_origin: Option<crate::SemanticsIndexOrigin>,

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

    /// Actions that can be performed on this node. Custom actions keep their
    /// handlers in `custom_handlers` instead, one per action.
    actions: FxHashMap<SemanticsAction, SemanticsActionHandler>,

    /// Each custom action's handler and the id its own configuration gave it,
    /// in the order of `attributes.custom_actions`, which holds their labels.
    /// A merge renumbers the published id; the handler still receives this one.
    custom_handlers: SmallVec<[(CustomActionId, SemanticsActionHandler); 2]>,

    /// What this node publishes (ADR-0206).
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
            && self.blocks_previous_siblings == other.blocks_previous_siblings
            && self.collection == other.collection
            && self.index_origin == other.index_origin
            && self.explicit_child_nodes == other.explicit_child_nodes
            && self.descendant_semantics_merge == other.descendant_semantics_merge
            && actions_are_identical
            && self.custom_handlers.len() == other.custom_handlers.len()
            && self
                .custom_handlers
                .iter()
                .zip(&other.custom_handlers)
                .all(|((a_id, a), (b_id, b))| a_id == b_id && Arc::ptr_eq(a, b))
            && self.attributes == other.attributes
    }
}

impl SemanticsConfiguration {
    /// Creates a new empty configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Retires action captures through an enclosing operation's failure custody.
    ///
    /// Each handler is retired independently; attributes and container buffers
    /// are destroyed normally. After a prior failure, opaque handlers are
    /// retained under ADR-0127 instead of invoking their destructors. On a
    /// healthy path a destructor failure stays in `recovery`, which decides
    /// when to propagate it. A single capture whose own fields double-panic
    /// remains outside that containment boundary.
    pub fn retire(self, recovery: &mut flui_foundation::panic::RecoveryScope<'_>) {
        let Self {
            actions,
            custom_handlers,
            attributes,
            ..
        } = self;
        for handler in actions.into_values() {
            recovery.retire(handler);
        }
        for (_, handler) in custom_handlers {
            recovery.retire(handler);
        }
        drop(attributes);
    }

    /// What this configuration publishes (ADR-0206).
    #[inline]
    #[must_use]
    pub(crate) fn attributes(&self) -> &SemanticsAttributes {
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
            && self.custom_handlers.is_empty()
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

    /// Whether this contribution hides preceding siblings in presentation order.
    /// The directive crosses transparent wrappers and stops at a formed boundary;
    /// it does not block this subtree's own labels or actions.
    pub fn blocks_previous_siblings(&self) -> bool {
        self.blocks_previous_siblings
    }

    /// Sets the preceding-sibling barrier without introducing a semantics node.
    pub fn set_blocks_previous_siblings(&mut self, value: bool) {
        self.blocks_previous_siblings = value;
    }

    /// Declares an explicit collection boundary with its logical member total.
    /// This never derives a total from the current resident children.
    pub fn set_collection(&mut self, collection: crate::SemanticsCollection) {
        self.describe_collection(collection, crate::SemanticsCollectionOrigin::Authored);
    }

    /// Declares a lazy host whose members carry that host's typed identity.
    pub fn set_lazy_collection(&mut self, collection: crate::SemanticsCollection) {
        self.describe_collection(collection, crate::SemanticsCollectionOrigin::Lazy);
    }

    fn describe_collection(
        &mut self,
        collection: crate::SemanticsCollection,
        origin: crate::SemanticsCollectionOrigin,
    ) {
        self.collection = Some((collection, origin));
        self.set_semantics_boundary(true);
        self.set_explicit_child_nodes(true);
        self.set_role(match collection.kind() {
            crate::SemanticsCollectionKind::List => SemanticsRole::List,
            crate::SemanticsCollectionKind::Grid => SemanticsRole::Grid,
        });
        self.attributes.scroll_child_count = collection
            .count()
            .and_then(|count| i32::try_from(count).ok());
    }

    /// The collection declared by this node itself, if any.
    #[must_use]
    pub fn collection(&self) -> Option<crate::SemanticsCollection> {
        self.collection.map(|(collection, _)| collection)
    }

    /// The source of this node's collection membership policy.
    #[must_use]
    pub fn collection_origin(&self) -> Option<crate::SemanticsCollectionOrigin> {
        self.collection.map(|(_, origin)| origin)
    }

    /// The declaration responsible for this node's member index.
    #[must_use]
    pub fn index_origin(&self) -> Option<crate::SemanticsIndexOrigin> {
        self.index_origin
    }

    /// Records an automatically stamped index belonging to `host`. The render
    /// assembler obtains that identity from the actual parent of a lazy slot.
    pub fn set_collection_index(&mut self, host: flui_foundation::RenderId, index: i32) {
        self.set_index_in_parent(index);
        self.index_origin = Some(crate::SemanticsIndexOrigin::Collection(host));
    }

    /// Withholds a managed total when an unrelated unknown member below it
    /// would otherwise inherit that total through the platform's ancestor walk.
    /// The original authored or lazy declaration remains available for subsequent assembly.
    pub fn omit_collection_total(&mut self) {
        if self.collection.is_some() {
            self.attributes.scroll_child_count = None;
        }
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
    ///
    /// A custom action is added with its label through
    /// [`Self::add_custom_action`]; a bare `CustomAction` handler names
    /// nothing to run and is not registered.
    pub fn add_action(&mut self, action: SemanticsAction, handler: SemanticsActionHandler) {
        if action == SemanticsAction::CustomAction {
            tracing::warn!("a custom action needs a label: use add_custom_action");
            return;
        }
        self.actions.insert(action, handler);
        self.mark_annotated();
    }

    /// Removes an action; `CustomAction` removes every custom action.
    pub fn remove_action(&mut self, action: SemanticsAction) {
        if action == SemanticsAction::CustomAction {
            // Every custom action goes: each label with its handler.
            self.attributes.custom_actions.clear();
            self.custom_handlers.clear();
        }
        self.actions.remove(&action);
        self.mark_annotated();
    }

    /// Returns whether an action is available.
    #[inline]
    pub fn has_action(&self, action: SemanticsAction) -> bool {
        self.actions.contains_key(&action)
            || (action == SemanticsAction::CustomAction && !self.custom_handlers.is_empty())
    }

    /// Returns the registered action handler for a given action.
    ///
    /// This intentionally exposes registration state for tree assembly. Code
    /// dispatching an action received from a platform must also verify that the
    /// action's bit is present in [`Self::effective_actions_as_bits`].
    pub fn action_handler(&self, action: SemanticsAction) -> Option<&SemanticsActionHandler> {
        self.actions.get(&action)
    }

    /// All registered standard and custom handler ownership, including blocked
    /// actions, for an assembler retaining captures until its pass is validated.
    ///
    /// This does not authorize dispatch. Platform action routing must still use
    /// the effective action mask and the current delivered offer.
    pub fn registered_action_handlers(&self) -> impl Iterator<Item = &SemanticsActionHandler> {
        self.actions
            .values()
            .chain(self.custom_handlers.iter().map(|(_, handler)| handler))
    }

    /// Record the ancestor reveal route discovered during render assembly.
    /// This structural directive does not annotate otherwise transparent nodes.
    pub fn set_has_reveal_ancestor(&mut self, value: bool) {
        self.has_reveal_ancestor = value;
    }

    /// Returns a bitmask of registered actions before blocking policy.
    pub fn actions_as_bits(&self) -> u64 {
        let custom = if self.custom_handlers.is_empty() {
            0
        } else {
            SemanticsAction::CustomAction.value()
        };
        self.actions
            .keys()
            .fold(custom, |acc, action| acc | action.value())
    }

    /// Returns the action bits that may be exposed to assistive technology.
    ///
    /// When user actions are blocked, every action but `Blur` is removed.
    /// Publication and action routing both use this policy rather than
    /// [`Self::actions_as_bits`].
    pub fn effective_actions_as_bits(&self) -> u64 {
        let actions = self.actions_as_bits()
            | if self.has_reveal_ancestor {
                SemanticsAction::ScrollIntoView.value()
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

    /// Adds a custom action, labelled as assistive technology offers it, with
    /// the handler that runs it, and returns the id it is published under.
    ///
    /// A node offers any number of custom actions, each with its own handler;
    /// the handler receives [`ActionCall::CustomAction`](crate::ActionCall::CustomAction)
    /// with this id.
    pub fn add_custom_action(
        &mut self,
        label: impl Into<SmolStr>,
        handler: SemanticsActionHandler,
    ) -> CustomActionId {
        let id = CustomActionId::at(self.custom_handlers.len());
        self.attributes.custom_actions.push(label.into());
        self.custom_handlers.push((id, handler));
        self.mark_annotated();
        id
    }

    /// The custom actions this node declares, with their ids, whether or not
    /// a block on user actions hides them.
    pub fn custom_actions(&self) -> impl Iterator<Item = (CustomActionId, &str)> + '_ {
        self.attributes
            .custom_actions
            .iter()
            .enumerate()
            .map(|(index, label)| (CustomActionId::at(index), label.as_str()))
    }

    /// The custom actions assistive technology may request: none while user
    /// actions are blocked.
    pub fn effective_custom_actions(&self) -> impl Iterator<Item = (CustomActionId, &str)> + '_ {
        let blocked = self.blocks_user_actions;
        self.custom_actions().filter(move |_| !blocked)
    }

    /// The custom action `id` names, when the node declares one and user
    /// actions are not blocked.
    pub fn custom_action(&self, id: CustomActionId) -> Option<DeclaredCustomAction<'_>> {
        if self.blocks_user_actions {
            return None;
        }
        let index = id.index();
        let (own_id, handler) = self.custom_handlers.get(index)?;
        Some(DeclaredCustomAction {
            label: self.attributes.custom_actions.get(index)?.as_str(),
            handler,
            own_id: *own_id,
        })
    }

    /// The custom actions assistive technology is offered, in id order: none
    /// while user actions are blocked.
    pub(crate) fn offered_custom_actions(&self) -> impl Iterator<Item = DeclaredCustomAction<'_>> {
        let offered = if self.blocks_user_actions {
            0
        } else {
            self.custom_handlers.len()
        };
        self.attributes
            .custom_actions
            .iter()
            .zip(&self.custom_handlers)
            .take(offered)
            .map(|(label, (own_id, handler))| DeclaredCustomAction {
                label: label.as_str(),
                handler,
                own_id: *own_id,
            })
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
        self.index_origin = Some(crate::SemanticsIndexOrigin::Authored);
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
        // A level belongs to a heading: a node that stops being one drops it,
        // so a heading it becomes again starts without a stale level.
        if role != SemanticsRole::Heading {
            self.attributes.heading_level = None;
        }
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
    /// `other` without dropping modeled information (ADR-0206, decision 4).
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
        // Two handlers for one action cannot share a node; custom actions each
        // carry their own handler, so any number of them can.
        let shared = self.actions_as_bits() & other.actions_as_bits();
        if shared & !SemanticsAction::CustomAction.value() != 0
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
            // A text group is one field's lengths.
            || both(
                max_value_length.is_some() || current_value_length.is_some(),
                theirs.max_value_length.is_some() || theirs.current_value_length.is_some(),
            ))
    }

    /// A role, or the route scope a page declares, which a second one
    /// cannot share a node with.
    fn has_explicit_role(&self) -> bool {
        self.attributes.role != SemanticsRole::None || self.scopes_route()
    }

    /// Absorbs the semantic information from another configuration.
    ///
    /// The merge rules, per group (ADR-0206, decision 4):
    ///
    /// - **States** — each by its own rule ([`States`]): `Mixed` over `On`
    ///   over `Off`, and a yes/no state as the receiver declares it, else as
    ///   the contributor does.
    /// - **Role** — by specificity, see `more_specific_role`; without a
    ///   role on either side, none.
    /// - **Actions** — absorb every action whose handler the child
    ///   defined. If `other.blocks_user_actions == true`, only actions in
    ///   the `UNBLOCKED_USER_ACTIONS_MASK` mask cross the boundary; the rest are
    ///   filtered out.
    /// - **Custom actions** — the contributor's follow the receiver's, each
    ///   with its own handler; a contributor that blocks user actions adds
    ///   none.
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

        // ----- actions (blocked / unblocked filter) -----
        let effective_other_actions = other.effective_actions_as_bits();
        for (action, handler) in &other.actions {
            if (action.value() & effective_other_actions) != 0 {
                self.actions.insert(*action, Arc::clone(handler));
            }
        }
        // Custom actions append, each with its own handler, so the
        // contributor's ids follow the receiver's. A contributor that blocks
        // user actions contributes none.
        if !other.blocks_user_actions {
            self.attributes
                .custom_actions
                .extend(other.attributes.custom_actions.iter().cloned());
            self.custom_handlers.extend(
                other
                    .custom_handlers
                    .iter()
                    .map(|(id, handler)| (*id, Arc::clone(handler))),
            );
        }

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
        if mine.index_in_parent.is_none()
            && mine.scroll_child_count.is_none()
            && other.collection.is_none()
        {
            mine.index_in_parent = *index_in_parent;
            mine.scroll_child_count = *scroll_child_count;
            self.index_origin = other.index_origin;
        }
        keep(&mut mine.platform_view_id, platform_view_id.as_ref());
        // A text group is copied whole, never mixed from two fields.
        if mine.max_value_length.is_none() && mine.current_value_length.is_none() {
            mine.max_value_length = *max_value_length;
            mine.current_value_length = *current_value_length;
        }
        keep(&mut mine.heading_level, heading_level.as_ref());
        // A level belongs to a heading, as in `set_role`: a merge whose role is
        // not one publishes no level and keeps none to revive.
        if mine.role != SemanticsRole::Heading {
            mine.heading_level = None;
        }

        self.has_been_annotated |= other.has_been_annotated;
    }
}

/// The role one node built from two takes (ADR-0206, decision 4).
///
/// A structural role (tab, list item, dialog, …) is authored structure and
/// outranks every interactive kind, as an explicit role always did. Among
/// interactive kinds the more specific wins: a checkable role (menu checkable
/// items included), then a button, then a link, text field, slider or other
/// value control (spin button, scroll bar, separator, combo box), then
/// content (an image, a heading, a read-only value such as a meter or
/// progress bar, or a text-level role such as a paragraph). On a tie the receiver keeps its role.
/// The order is what makes a merged list tile with a radio a radio button
/// rather than a button
/// (`merge_semantics_over_a_tile_and_radio_announces_as_one_radio_button`).
pub(crate) fn more_specific_role(
    receiver: SemanticsRole,
    contributor: SemanticsRole,
) -> SemanticsRole {
    fn rank(role: SemanticsRole) -> u8 {
        match role {
            SemanticsRole::None => 0,
            SemanticsRole::Image
            | SemanticsRole::Heading
            | SemanticsRole::Paragraph
            | SemanticsRole::Caption
            | SemanticsRole::Code
            | SemanticsRole::Emphasis
            | SemanticsRole::Strong
            | SemanticsRole::Insertion
            | SemanticsRole::Deletion
            | SemanticsRole::Term
            | SemanticsRole::Definition
            | SemanticsRole::Time
            | SemanticsRole::Math
            | SemanticsRole::Meter
            | SemanticsRole::ProgressBar => 1,
            SemanticsRole::Link
            | SemanticsRole::Slider
            | SemanticsRole::SpinButton
            | SemanticsRole::ScrollBar
            | SemanticsRole::Separator
            | SemanticsRole::ComboBox => 2,
            role if role.is_text_input() => 2,
            SemanticsRole::Button => 3,
            SemanticsRole::CheckBox
            | SemanticsRole::RadioButton
            | SemanticsRole::Switch
            | SemanticsRole::MenuItemCheckbox
            | SemanticsRole::MenuItemRadio => 4,
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
        tap.add_action(SemanticsAction::Click, Arc::new(|_| {}));
        let mut long_press = SemanticsConfiguration::new();
        long_press.add_action(SemanticsAction::ShowContextMenu, Arc::new(|_| {}));
        let mut another_tap = SemanticsConfiguration::new();
        another_tap.add_action(SemanticsAction::Click, Arc::new(|_| {}));
        assert_compatibility_is_symmetric(&tap, &long_press, true);
        assert_compatibility_is_symmetric(&tap, &another_tap, false);
        // Custom actions each carry their own handler: two sets share a node.
        let mut archive = SemanticsConfiguration::new();
        archive.add_custom_action("Archive", Arc::new(|_| {}));
        let mut delete = SemanticsConfiguration::new();
        delete.add_custom_action("Delete", Arc::new(|_| {}));
        assert_compatibility_is_symmetric(&archive, &delete, true);

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
        let mut limit = SemanticsConfiguration::new();
        limit.set_max_value_length(10);
        let mut length = SemanticsConfiguration::new();
        length.set_current_value_length(3);
        assert_compatibility_is_symmetric(&limit, &length, false);

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
        for role in [
            SemanticsRole::Button,
            SemanticsRole::TextInput,
            SemanticsRole::Slider,
            SemanticsRole::Link,
            SemanticsRole::Image,
            SemanticsRole::Heading,
        ] {
            let mut second = SemanticsConfiguration::new();
            second.set_role(role);
            assert_compatibility_is_symmetric(&dialog, &second, false);
        }
        let mut route = SemanticsConfiguration::new();
        route.set_scopes_route(true);
        assert_compatibility_is_symmetric(&dialog, &route, false);

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

    /// A forced merge offers both sides' custom actions, the contributor's
    /// renumbered after the receiver's; each keeps the handler it was added
    /// with and the id its own configuration gave it. A contributor that
    /// blocks user actions adds none.
    #[test]
    fn a_forced_merge_keeps_custom_actions_with_their_handler() {
        let ran = Arc::new(std::sync::Mutex::new(Vec::new()));
        let source = |side: &'static str, labels: &[&'static str]| {
            let mut config = SemanticsConfiguration::new();
            for &label in labels {
                let ran = Arc::clone(&ran);
                config.add_custom_action(
                    label,
                    Arc::new(move |_| ran.lock().expect("log").push((side, label))),
                );
            }
            config
        };
        let mut merged = source("receiver", &["Archive", "Pin"]);
        merged.absorb(&source("contributor", &["Delete", "Pin"]));

        let published: Vec<(CustomActionId, &str)> = merged.effective_custom_actions().collect();
        assert_eq!(
            published,
            [
                (CustomActionId::at(0), "Archive"),
                (CustomActionId::at(1), "Pin"),
                (CustomActionId::at(2), "Delete"),
                (CustomActionId::at(3), "Pin"),
            ],
            "both sides' actions are offered, the contributor's renumbered after"
        );
        let own: Vec<CustomActionId> = published
            .iter()
            .map(|&(id, _)| {
                let declared = merged.custom_action(id).expect("a declared id");
                (declared.handler)(crate::ActionCall::CustomAction(declared.own_id));
                declared.own_id
            })
            .collect();
        assert_eq!(
            own,
            [0, 1, 0, 1].map(CustomActionId::at),
            "each action keeps the id its own configuration gave it"
        );
        assert_eq!(
            *ran.lock().expect("log"),
            [
                ("receiver", "Archive"),
                ("receiver", "Pin"),
                ("contributor", "Delete"),
                ("contributor", "Pin"),
            ],
            "each action runs the handler it was added with"
        );

        let mut blocked = source("contributor", &["Delete"]);
        blocked.set_blocks_user_actions(true);
        let mut merged = source("receiver", &["Archive"]);
        merged.absorb(&blocked);
        assert_eq!(
            merged.custom_actions().count(),
            1,
            "a blocked part adds none"
        );
    }

    /// Removing `CustomAction` removes every custom action, label and handler
    /// together, so none stays published or invocable.
    #[test]
    fn removing_the_custom_action_removes_every_one() {
        let mut config = SemanticsConfiguration::new();
        config.add_custom_action("Archive", Arc::new(|_| {}));
        config.add_custom_action("Pin", Arc::new(|_| {}));
        config.remove_action(SemanticsAction::CustomAction);
        assert!(!config.has_action(SemanticsAction::CustomAction));
        assert_eq!(config.custom_actions().count(), 0);
        assert!(config.custom_action(CustomActionId::at(0)).is_none());
    }

    /// A focusable node that offers a custom action is a real control, not a
    /// bare focus annotation, however its handlers are stored.
    #[test]
    fn a_custom_action_is_more_than_focus_state() {
        let mut config = SemanticsConfiguration::new();
        config.set_focusable(true);
        assert!(config.is_focus_state_only());
        config.add_custom_action("Archive", Arc::new(|_| {}));
        assert!(!config.is_focus_state_only());
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

    /// The collection and text groups merge like the scroll group: one
    /// entry's place in one set, one field's lengths, never mixed from two.
    #[test]
    fn a_forced_merge_keeps_one_collection_and_text_group() {
        type Setter = fn(&mut SemanticsConfiguration, i32);
        type Getter = fn(&SemanticsConfiguration) -> Option<i32>;
        type Field = (Setter, Getter);
        let groups: &[(&str, Field, Field)] = &[
            (
                "collection",
                (
                    SemanticsConfiguration::set_index_in_parent,
                    SemanticsConfiguration::index_in_parent,
                ),
                (
                    SemanticsConfiguration::set_scroll_child_count,
                    SemanticsConfiguration::scroll_child_count,
                ),
            ),
            (
                "text",
                (
                    SemanticsConfiguration::set_current_value_length,
                    SemanticsConfiguration::current_value_length,
                ),
                (
                    SemanticsConfiguration::set_max_value_length,
                    SemanticsConfiguration::max_value_length,
                ),
            ),
        ];
        for &(group, (set_first, first), (set_second, second)) in groups {
            let mut receiver = SemanticsConfiguration::new();
            set_first(&mut receiver, 1);
            let mut contributor = SemanticsConfiguration::new();
            set_first(&mut contributor, 7);
            set_second(&mut contributor, 9);
            assert!(
                !receiver.is_compatible_with(&contributor),
                "two {group} groups form separate nodes"
            );

            let mut kept = receiver.clone();
            kept.absorb(&contributor);
            assert_eq!(
                (first(&kept), second(&kept)),
                (Some(1), None),
                "the receiver's {group} group is not mixed with the contributor's"
            );

            let mut taken = SemanticsConfiguration::new();
            taken.set_label("receiver");
            taken.absorb(&contributor);
            assert_eq!(
                (first(&taken), second(&taken)),
                (Some(7), Some(9)),
                "a receiver without a {group} group takes the contributor's whole"
            );
        }
    }

    /// A forced merge takes each state by its type's rule: `Mixed` over `On`
    /// over `Off`, and a yes/no state as the receiver declares it, or as the
    /// contributor does when the receiver declares nothing.
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
        let unfocused = merged(|c| c.set_focused(false), |c| c.set_focused(true));
        assert!(!unfocused.is_focused(), "the receiver's declaration stands");
        let disabled = merged(
            |c| c.set_enabled(Some(false)),
            |c| c.set_enabled(Some(true)),
        );
        assert_eq!(
            disabled.is_enabled(),
            Some(false),
            "a disabled receiver is not enabled by a part"
        );
        let enabled = merged(|_| {}, |c| c.set_enabled(Some(true)));
        assert_eq!(
            enabled.is_enabled(),
            Some(true),
            "an undeclared state takes the contributor's"
        );
        let kept = merged(|c| c.set_enabled(Some(false)), |_| {});
        assert_eq!(kept.is_enabled(), Some(false));
    }

    /// A level belongs to a heading: a node that becomes something else drops
    /// it, so a heading it becomes again is announced without a stale level.
    #[test]
    fn a_role_change_drops_a_heading_level() {
        let mut config = SemanticsConfiguration::new();
        config.set_heading_level(HeadingLevel::H2);
        config.set_role(SemanticsRole::Heading);
        assert_eq!(
            config.heading_level(),
            Some(HeadingLevel::H2),
            "still a heading"
        );
        config.set_role(SemanticsRole::Button);
        config.set_role(SemanticsRole::Heading);
        assert_eq!(config.heading_level(), None);

        // The same holds when a merge settles the role: a heading that absorbs
        // a button becomes the button, with no level to publish or revive.
        let mut heading = SemanticsConfiguration::new();
        heading.set_heading_level(HeadingLevel::H2);
        let mut button = SemanticsConfiguration::new();
        button.set_role(SemanticsRole::Button);
        heading.absorb(&button);
        assert_eq!(heading.role(), SemanticsRole::Button);
        assert_eq!(
            heading.heading_level(),
            None,
            "a merged button has no level"
        );
        button.absorb(&{
            let mut h3 = SemanticsConfiguration::new();
            h3.set_heading_level(HeadingLevel::H3);
            h3
        });
        assert_eq!(button.heading_level(), None, "nor takes a heading's");
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
            (R::ScrollBar, R::CheckBox, R::CheckBox),
            (R::SpinButton, R::Switch, R::Switch),
            (R::ComboBox, R::Button, R::Button),
            (R::Separator, R::Heading, R::Separator),
            (R::MenuItemCheckbox, R::Button, R::MenuItemCheckbox),
            (R::Button, R::Meter, R::Button),
            (R::ProgressBar, R::CheckBox, R::CheckBox),
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
    fn a_blocked_node_keeps_only_blur() {
        for &action in SemanticsAction::ALL {
            let mut config = SemanticsConfiguration::new();
            config.add_action(action, Arc::new(|_| {}));
            config.set_blocks_user_actions(true);

            let expected = if action == SemanticsAction::Blur {
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
