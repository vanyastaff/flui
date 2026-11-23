//! The `Semantics` widget: a subtree annotated with accessibility semantics, its
//! builders, and the owner-lane registration of its action table.

#[cfg(doc)]
use super::MergeSemantics;
use std::any::Any;
use std::num::{NonZeroU32, NonZeroUsize};
use std::rc::Rc;

use flui_foundation::CreationSite;
use flui_foundation::geometry::{Axis, Offset};
use flui_foundation::{ChangeNotifier, TextDirection};
use flui_interaction::pointer::{InteractionDispatchError, LocalPayloadTarget};
use flui_objects::{GatedCustomAction, RenderSemanticsAnnotations, SemanticsActionRoute};
use flui_rendering::protocol::BoxProtocol;
use flui_semantics::{
    ActionCall, AutoComplete, CellPosition, Current, HasPopup, HeadingLevel, Invalid, LiveSetting,
    Reveal, ScrollUnit, SemanticsAction, SemanticsActionHandler, SemanticsAnchor,
    SemanticsConfiguration, SemanticsRelation, SemanticsRole, SetSize, SortDirection, TableShape,
    Toggled,
};
use flui_view::{
    Child, EventContext, EventOutcome, IntoView, RenderObjectContext, RenderObjectContextError,
    RenderView, impl_render_view,
};

use crate::support::{ref_callback, value_callback};

use super::actions::{
    CustomEntry, EventActionHandler, EventActions, SemanticsActionCell, deliver,
    deliver_assistive_focus,
};

#[derive(Clone, Copy, Debug, Default)]
struct SemanticsOptions {
    bits: u8,
}

impl SemanticsOptions {
    const CONTAINER: u8 = 1 << 0;
    const EXPLICIT_CHILD_NODES: u8 = 1 << 1;
    const EXCLUDE_DESCENDANTS: u8 = 1 << 2;
    const BLOCK_USER_ACTIONS: u8 = 1 << 3;

    #[inline]
    const fn contains(self, flag: u8) -> bool {
        (self.bits & flag) != 0
    }

    #[inline]
    fn set(&mut self, flag: u8, value: bool) {
        if value {
            self.bits |= flag;
        } else {
            self.bits &= !flag;
        }
    }
}

/// Annotates a subtree with accessibility semantics.
#[derive(Clone)]
pub struct Semantics {
    configuration: SemanticsConfiguration,
    event_actions: EventActions,
    refresh: Vec<ChangeNotifier>,
    options: SemanticsOptions,
    scroll: Option<(
        flui_rendering::view::ScrollPosition,
        flui_foundation::geometry::AxisDirection,
    )>,
    child: Child,
    site: CreationSite,
}

impl std::fmt::Debug for Semantics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Semantics")
            .field("configuration", &self.configuration)
            .field("scroll", &self.scroll)
            .field("event_actions", &self.event_actions)
            .field("refresh_sources", &self.refresh.len())
            .field("options", &self.options)
            .field("child", &self.child)
            .finish_non_exhaustive()
    }
}

impl Default for Semantics {
    fn default() -> Self {
        // A trait fn cannot be `#[track_caller]`: the site is this line.
        Self::at(SemanticsConfiguration::new(), CreationSite::here())
    }
}

impl Semantics {
    fn at(configuration: SemanticsConfiguration, site: CreationSite) -> Self {
        Self {
            configuration,
            event_actions: EventActions::default(),
            refresh: Vec::new(),
            options: SemanticsOptions::default(),
            scroll: None,
            child: Child::empty(),
            site,
        }
    }

    /// Creates an empty semantics annotation.
    #[track_caller]
    pub fn new() -> Self {
        Self::at(SemanticsConfiguration::new(), CreationSite::here())
    }

    /// Creates a semantics annotation from a ready configuration.
    #[track_caller]
    pub fn from_configuration(configuration: SemanticsConfiguration) -> Self {
        Self::at(configuration, CreationSite::here())
    }
    pub(crate) fn scroll_source(
        mut self,
        position: flui_rendering::view::ScrollPosition,
        direction: flui_foundation::geometry::AxisDirection,
    ) -> Self {
        self.scroll = Some((position, direction));
        self
    }

    /// Set whether this widget introduces a new semantics node.
    #[must_use]
    pub fn container(mut self, container: bool) -> Self {
        self.options.set(SemanticsOptions::CONTAINER, container);
        self
    }

    /// Set whether descendants must create explicit semantics nodes.
    #[must_use]
    pub fn explicit_child_nodes(mut self, explicit_child_nodes: bool) -> Self {
        self.options
            .set(SemanticsOptions::EXPLICIT_CHILD_NODES, explicit_child_nodes);
        self
    }

    /// Set whether descendant semantics are ignored.
    #[must_use]
    pub fn exclude_semantics(mut self, exclude_semantics: bool) -> Self {
        self.options
            .set(SemanticsOptions::EXCLUDE_DESCENDANTS, exclude_semantics);
        self
    }

    /// Set whether user-action semantics are blocked for descendants.
    #[must_use]
    pub fn block_user_actions(mut self, block_user_actions: bool) -> Self {
        self.options
            .set(SemanticsOptions::BLOCK_USER_ACTIONS, block_user_actions);
        self
    }

    /// Hide preceding siblings in presentation order up to the nearest semantics
    /// boundary. This subtree and later siblings keep their labels and actions.
    #[must_use]
    pub fn block_previous_siblings(mut self, blocking: bool) -> Self {
        self.configuration.set_blocks_previous_siblings(blocking);
        self
    }

    /// Set the accessible label.
    #[must_use]
    pub fn label(mut self, label: impl AsRef<str>) -> Self {
        self.configuration.set_label(label);
        self
    }

    /// Set the stable identifier tests and automation select this node by
    /// (ADR-0240), exported as UI Automation's `AutomationId`, macOS
    /// `AXIdentifier` and AT-SPI's accessible id. It is never announced, and
    /// it does not make a node: it names this widget's node only when the
    /// widget forms one ([`container`](Self::container), or under a parent
    /// that requires explicit child nodes). Absorbed into an ancestor or
    /// merged by `MergeSemantics`, it is dropped, not carried up. A node with
    /// no role, label or text, or with the `Generic` or `Presentation` role,
    /// is lifted into its parent by the agent and the AccessKit adapters, its
    /// identifier with it: to select a widget with no semantics of its own,
    /// give the node a role as well, as `.role(SemanticsRole::Group)`. Inside
    /// a control such as a button, give the identifier to the control
    /// instead: a group there would cut the name its label gives it.
    #[must_use]
    pub fn identifier(mut self, identifier: impl Into<std::sync::Arc<str>>) -> Self {
        self.configuration.set_identifier(identifier);
        self
    }

    /// [`identifier`](Self::identifier) when `identifier` is `Some`: how a
    /// control forwards the one its author gave it.
    #[must_use]
    pub(crate) fn identifier_if(self, identifier: Option<&std::sync::Arc<str>>) -> Self {
        match identifier {
            Some(identifier) => self.identifier(std::sync::Arc::clone(identifier)),
            None => self,
        }
    }

    /// Set the accessible value.
    #[must_use]
    pub fn value(mut self, value: impl AsRef<str>) -> Self {
        self.configuration.set_value(value);
        self
    }

    /// Set the accessible hint.
    #[must_use]
    pub fn hint(mut self, hint: impl AsRef<str>) -> Self {
        self.configuration.set_hint(hint);
        self
    }

    /// Set whether this node is enabled.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.configuration.set_enabled(Some(enabled));
        self
    }

    /// Make this node a check box in `state`.
    #[must_use]
    pub fn checkbox(mut self, state: Toggled) -> Self {
        self.configuration.set_role(SemanticsRole::CheckBox);
        self.configuration.set_toggled(Some(state));
        self
    }

    /// Make this node one option of a mutually exclusive set, checked or not.
    #[must_use]
    pub fn radio(mut self, checked: bool) -> Self {
        self.configuration.set_role(SemanticsRole::RadioButton);
        self.configuration
            .set_toggled(Some(Toggled::from_bool(checked)));
        self
    }

    /// Make this node an on/off switch.
    #[must_use]
    pub fn switch(mut self, on: bool) -> Self {
        self.configuration.set_role(SemanticsRole::Switch);
        self.configuration.set_toggled(Some(Toggled::from_bool(on)));
        self
    }

    /// Set the checked state of a checkable role, or the pressed state of a
    /// button: the node's role says which.
    #[must_use]
    pub fn toggled(mut self, state: Toggled) -> Self {
        self.configuration.set_toggled(Some(state));
        self
    }

    /// Set whether this node is selected.
    #[must_use]
    pub fn selected(mut self, selected: bool) -> Self {
        self.configuration.set_selected(selected);
        self
    }

    /// Set whether this node has expanded state and is expanded.
    #[must_use]
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.configuration.set_expanded(expanded);
        self
    }

    /// Make this node a header at `level` of the outline, which screen
    /// readers announce and navigate by.
    #[must_use]
    pub fn heading_level(mut self, level: HeadingLevel) -> Self {
        self.configuration.set_heading_level(level);
        self
    }

    /// Set this node's zero-based place among the members of its set: the
    /// "12" of "item 12 of 100" is `11`.
    ///
    /// A lazy list numbers its own rows; this is for content a caller owns, a
    /// hand-built list or a grid of cards. The size of the set belongs to the
    /// container ([`Self::size_of_set`]).
    #[must_use]
    pub fn position_in_set(mut self, index: usize) -> Self {
        self.configuration.set_position_in_set(index);
        self
    }

    /// Set how many members the set this node contains has. A set whose
    /// length is not known says [`SetSize::Unknown`]: the number of members
    /// built so far is not its size.
    #[must_use]
    pub fn size_of_set(mut self, size: SetSize) -> Self {
        self.configuration.set_size_of_set(size);
        self
    }

    /// Set the depth of this tree item, list item or row; `1` is the top level.
    #[must_use]
    pub fn level(mut self, level: NonZeroU32) -> Self {
        self.configuration.set_level(level);
        self
    }

    /// Set the number of rows and columns of the table, grid or tree grid this
    /// node is.
    #[must_use]
    pub fn table_shape(mut self, shape: TableShape) -> Self {
        self.configuration.set_table_shape(shape);
        self
    }

    /// Set where this cell, row or header sits in its table, counted from one
    /// as `aria-rowindex` and `aria-colindex` are. A virtualized table gives
    /// each built cell its true place, not its place among the built.
    #[must_use]
    pub fn cell_position(mut self, position: CellPosition) -> Self {
        self.configuration.set_cell_position(position);
        self
    }

    /// Set this row's own place in its table, counted from one as
    /// `aria-rowindex` is. A cell or a header declares its row and column
    /// together with [`Self::cell_position`].
    #[must_use]
    pub fn row_index(mut self, row: NonZeroUsize) -> Self {
        self.configuration.set_row_index(row);
        self
    }

    /// Set the order the column or row this header heads is sorted in.
    #[must_use]
    pub fn sort_direction(mut self, direction: SortDirection) -> Self {
        self.configuration.set_sort_direction(direction);
        self
    }

    /// Set whether the list, tree, grid or tab list this node is allows more
    /// than one selected member.
    #[must_use]
    pub fn multiselectable(mut self, multiselectable: bool) -> Self {
        self.configuration.set_multiselectable(multiselectable);
        self
    }

    /// Set whether this node is read-only.
    #[must_use]
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.configuration.set_read_only(read_only);
        self
    }

    /// Set whether this node is focusable.
    #[must_use]
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.configuration.set_focusable(focusable);
        self
    }

    /// Set whether this node is focused.
    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.configuration.set_focused(focused);
        self
    }

    /// Set whether this node is hidden from accessibility.
    #[must_use]
    pub fn hidden(mut self, hidden: bool) -> Self {
        self.configuration.set_hidden(hidden);
        self
    }

    /// Set whether this node scopes a route.
    #[must_use]
    pub fn scopes_route(mut self, scopes_route: bool) -> Self {
        self.configuration.set_scopes_route(scopes_route);
        self
    }

    /// Set whether this node names a route.
    #[must_use]
    pub fn names_route(mut self, names_route: bool) -> Self {
        self.configuration.set_names_route(names_route);
        self
    }

    /// Name this node by `anchor`, so other nodes' relations to the anchor
    /// resolve here (ADR-0205).
    #[must_use]
    pub fn anchor(mut self, anchor: SemanticsAnchor) -> Self {
        self.configuration.add_anchor(anchor);
        self
    }

    /// Name this node by the nodes `anchor` names, when it has no label of
    /// its own: a dialog by its title.
    #[must_use]
    pub fn labelled_by(mut self, anchor: SemanticsAnchor) -> Self {
        self.configuration
            .add_relation(SemanticsRelation::LabelledBy, anchor);
        self
    }

    /// Record that this node controls the nodes `anchor` names: a button and
    /// the panel it opens.
    #[must_use]
    pub fn controls(mut self, anchor: SemanticsAnchor) -> Self {
        self.configuration
            .add_relation(SemanticsRelation::Controls, anchor);
        self
    }

    /// Record that the nodes `anchor` names describe this one: a field's
    /// helper line, read after its name.
    #[must_use]
    pub fn described_by(mut self, anchor: SemanticsAnchor) -> Self {
        self.configuration
            .add_relation(SemanticsRelation::DescribedBy, anchor);
        self
    }

    /// Record that the nodes `anchor` names hold more detail about this one:
    /// a chart's data table.
    #[must_use]
    pub fn details(mut self, anchor: SemanticsAnchor) -> Self {
        self.configuration
            .add_relation(SemanticsRelation::Details, anchor);
        self
    }

    /// Record that reading continues at the nodes `anchor` names after this
    /// one, overriding the order of the tree.
    #[must_use]
    pub fn flow_to(mut self, anchor: SemanticsAnchor) -> Self {
        self.configuration
            .add_relation(SemanticsRelation::FlowTo, anchor);
        self
    }

    /// Record that the node `anchor` names says why this field's value is
    /// invalid.
    #[must_use]
    pub fn error_message(mut self, anchor: SemanticsAnchor) -> Self {
        self.configuration
            .add_relation(SemanticsRelation::ErrorMessage, anchor);
        self
    }

    /// Record that the node `anchor` names is the active descendant of this
    /// focused composite: the highlighted option of a combo box whose focus
    /// stays in its field.
    #[must_use]
    pub fn active_descendant(mut self, anchor: SemanticsAnchor) -> Self {
        self.configuration
            .add_relation(SemanticsRelation::ActiveDescendant, anchor);
        self
    }

    /// Set whether this form control must be filled before submitting.
    #[must_use]
    pub fn required(mut self, required: bool) -> Self {
        self.configuration.set_required(required);
        self
    }

    /// Declare whether this form control's value failed validation, and how;
    /// [`Invalid::False`] declares it validated and passed.
    #[must_use]
    pub fn invalid(mut self, invalid: Invalid) -> Self {
        self.configuration.set_invalid(invalid);
        self
    }

    /// Declare this node the current item of its set: the page a navigation
    /// link leads to, the step of a process.
    #[must_use]
    pub fn current(mut self, current: Current) -> Self {
        self.configuration.set_current(current);
        self
    }

    /// Declare what this node opens when activated.
    #[must_use]
    pub fn has_popup(mut self, popup: HasPopup) -> Self {
        self.configuration.set_has_popup(popup);
        self
    }

    /// Declare how this text field or combo box completes typed text.
    #[must_use]
    pub fn auto_complete(mut self, auto_complete: AutoComplete) -> Self {
        self.configuration.set_auto_complete(auto_complete);
        self
    }

    /// Declare the axis this control runs along.
    #[must_use]
    pub fn orientation(mut self, orientation: Axis) -> Self {
        self.configuration.set_orientation(orientation);
        self
    }

    /// Set the keys that activate or focus this node, in the
    /// `aria-keyshortcuts` grammar (`"Control+S"`).
    #[must_use]
    pub fn keyboard_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.configuration.set_keyboard_shortcut(shortcut.into());
        self
    }

    /// Set the BCP 47 language of this node's text (`"fr-FR"`).
    #[must_use]
    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.configuration.set_language(language.into());
        self
    }

    /// Set the address a link leads to or an image came from.
    #[must_use]
    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.configuration.set_url(url.into());
        self
    }

    /// Set the words a screen reader says for this node's role ("slide").
    #[must_use]
    pub fn role_description(mut self, description: impl Into<String>) -> Self {
        self.configuration.set_role_description(description.into());
        self
    }

    /// Set what an empty text field shows in place of a value.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.configuration.set_placeholder(placeholder.into());
        self
    }

    /// Set whether this node is modal, as a dialog is: while it is shown,
    /// assistive technology reaches only it and its descendants.
    #[must_use]
    pub fn modal(mut self, modal: bool) -> Self {
        self.configuration.set_modal(modal);
        self
    }

    /// Make this node a live region: assistive technology announces its
    /// changes without the user moving to it, politely or interrupting.
    /// [`LiveSetting::Off`] keeps it silent inside an enclosing region.
    #[must_use]
    pub fn live_region(mut self, setting: LiveSetting) -> Self {
        self.configuration.set_live_region(setting);
        self
    }

    /// Set whether a change announces the whole region rather than only what
    /// changed. Undeclared, an alert or a status is atomic.
    #[must_use]
    pub fn live_atomic(mut self, atomic: bool) -> Self {
        self.configuration.set_live_atomic(atomic);
        self
    }

    /// Set whether this node is being updated: a screen reader holds back
    /// the announcements of its subtree until it is not.
    #[must_use]
    pub fn busy(mut self, busy: bool) -> Self {
        self.configuration.set_busy(busy);
        self
    }

    /// Set the text direction used when merging text semantics.
    #[must_use]
    pub fn text_direction(mut self, text_direction: TextDirection) -> Self {
        self.configuration.set_text_direction(text_direction);
        self
    }

    /// Set the platform semantics role.
    #[must_use]
    pub fn role(mut self, role: SemanticsRole) -> Self {
        self.configuration.set_role(role);
        self
    }

    // -----------------------------------------------------------------------
    // Actions
    // -----------------------------------------------------------------------

    /// Invoke `handler` when assistive technology activates this node.
    ///
    /// The action a screen reader's activate gesture produces, advertised to
    /// the platform as a click — this is what makes a custom-drawn control
    /// pressable without a pointer.
    #[must_use]
    pub fn on_click<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_plain_action(SemanticsAction::Click, handler)
    }

    /// Invoke `handler` when assistive technology asks for this node's context
    /// menu.
    #[must_use]
    pub fn on_show_context_menu<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_plain_action(SemanticsAction::ShowContextMenu, handler)
    }

    /// Invoke `handler` when assistive technology scrolls this node left, by
    /// the [`ScrollUnit`] it asked for.
    #[must_use]
    pub fn on_scroll_left<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>, ScrollUnit) -> R + 'static,
        R: EventOutcome,
    {
        self.on_scroll_action(SemanticsAction::ScrollLeft, handler)
    }

    /// Invoke `handler` when assistive technology scrolls this node right, by
    /// the [`ScrollUnit`] it asked for.
    #[must_use]
    pub fn on_scroll_right<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>, ScrollUnit) -> R + 'static,
        R: EventOutcome,
    {
        self.on_scroll_action(SemanticsAction::ScrollRight, handler)
    }

    /// Invoke `handler` when assistive technology scrolls this node up, by the
    /// [`ScrollUnit`] it asked for.
    #[must_use]
    pub fn on_scroll_up<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>, ScrollUnit) -> R + 'static,
        R: EventOutcome,
    {
        self.on_scroll_action(SemanticsAction::ScrollUp, handler)
    }

    /// Invoke `handler` when assistive technology scrolls this node down, by
    /// the [`ScrollUnit`] it asked for.
    #[must_use]
    pub fn on_scroll_down<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>, ScrollUnit) -> R + 'static,
        R: EventOutcome,
    {
        self.on_scroll_action(SemanticsAction::ScrollDown, handler)
    }

    /// Registers a directional scroll handler, which receives the call's unit.
    fn on_scroll_action<F, R>(mut self, action: SemanticsAction, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>, ScrollUnit) -> R + 'static,
        R: EventOutcome,
    {
        let handler = value_callback(handler);
        self.event_actions.insert(
            action,
            Rc::new(move |ctx, call| match call {
                ActionCall::ScrollUp(unit)
                | ActionCall::ScrollDown(unit)
                | ActionCall::ScrollLeft(unit)
                | ActionCall::ScrollRight(unit) => handler(ctx, unit),
                _ => {}
            }),
        );
        self
    }

    /// Publishes a validated numeric range for native range-value controls.
    ///
    /// A node with no role of its own becomes a slider; a node that declares
    /// one (a spin button, a progress bar) keeps it.
    #[must_use]
    pub fn numeric_range(mut self, range: flui_semantics::NumericRange) -> Self {
        self.configuration.set_numeric_range(range);
        self
    }

    /// Publishes whether this control is expanded, with the handlers that
    /// assistive technology's expand and collapse requests reach.
    ///
    /// The state and the handlers come together because a platform offers
    /// only the transition the published state allows (`Expand` while
    /// collapsed, `Collapse` while expanded) and the owner refuses the other:
    /// a handler on a node without an expanded state would be registered and
    /// never reachable. Each handler must set the requested state, including a
    /// repeated request queued before the next frame republishes it.
    ///
    /// For a control that toggles through its click handler alone, publish the
    /// state with [`Self::expanded`] and register [`Self::on_click`]: that
    /// handler then serves whichever transition the state allows.
    ///
    /// There is no separate expand or collapse handler to attach without a
    /// state:
    ///
    /// ```compile_fail
    /// let _ = flui_widgets::Semantics::new().on_expand(|_ctx| {});
    /// ```
    ///
    /// Register the expanded state and both transition handlers together:
    ///
    /// ```
    /// let _ = flui_widgets::Semantics::new().expandable(false, |_ctx| {}, |_ctx| {});
    /// ```
    #[must_use]
    pub fn expandable<E, RE, C, RC>(self, expanded: bool, on_expand: E, on_collapse: C) -> Self
    where
        E: Fn(&mut EventContext<'_>) -> RE + 'static,
        RE: EventOutcome,
        C: Fn(&mut EventContext<'_>) -> RC + 'static,
        RC: EventOutcome,
    {
        self.expanded(expanded)
            .on_plain_action(SemanticsAction::Expand, on_expand)
            .on_plain_action(SemanticsAction::Collapse, on_collapse)
    }

    /// Receives the exact finite numeric value admitted by the current range.
    /// Step metadata does not quantize assistive-technology values.
    #[must_use]
    pub fn on_set_numeric_value<F, R>(mut self, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>, f64) -> R + 'static,
        R: EventOutcome,
    {
        let handler = value_callback(handler);
        self.event_actions.insert(
            SemanticsAction::SetNumericValue,
            Rc::new(move |ctx, call| {
                if let ActionCall::SetNumericValue(value) = call {
                    handler(ctx, value);
                }
            }),
        );
        self
    }

    /// Invoke `handler` when assistive technology raises this node's value
    /// one step.
    ///
    /// The counterpart to [`Self::on_decrement`], for sliders and steppers
    /// whose value a screen reader can adjust without a pointer.
    #[must_use]
    pub fn on_increment<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_plain_action(SemanticsAction::Increment, handler)
    }

    /// Invoke `handler` when assistive technology lowers this node's value
    /// one step.
    #[must_use]
    pub fn on_decrement<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_plain_action(SemanticsAction::Decrement, handler)
    }

    /// Invoke `handler` when assistive technology asks for something to be
    /// brought into view: this node itself (`None`), or, for a scrollable, a
    /// descendant whose geometry the owner resolved ([`Reveal`]).
    ///
    /// How a screen reader moves a scrollable to whatever it is reading
    /// *next*, which is not always something the pointer path can trigger —
    /// the platform asks for a node that is currently offscreen.
    #[must_use]
    pub fn on_scroll_into_view<F, R>(mut self, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>, Option<Reveal>) -> R + 'static,
        R: EventOutcome,
    {
        let handler = value_callback(handler);
        self.event_actions.insert(
            SemanticsAction::ScrollIntoView,
            Rc::new(move |ctx, call| {
                if let ActionCall::ScrollIntoView(reveal) = call {
                    handler(ctx, reveal);
                }
            }),
        );
        self
    }

    /// Invoke `handler` when assistive technology moves keyboard focus to this
    /// node.
    #[must_use]
    pub fn on_focus<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_plain_action(SemanticsAction::Focus, handler)
    }

    /// Invoke `handler` when assistive technology asks this node to give up
    /// keyboard focus.
    ///
    /// The one action a node with blocked user actions still offers: a
    /// control the user cannot operate can still be left.
    #[must_use]
    pub fn on_blur<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_plain_action(SemanticsAction::Blur, handler)
    }

    /// Invoke `handler` with the text a platform asked this node to hold.
    ///
    /// How a screen reader enters text into a custom-drawn field. A request
    /// without its text never reaches here: it is refused when it is decoded.
    #[must_use]
    pub fn on_set_value<F, R>(mut self, handler: F) -> Self
    where
        F: for<'a> Fn(&mut EventContext<'_>, &'a str) -> R + 'static,
        R: EventOutcome,
    {
        let handler = ref_callback::<str, _, _>(handler);
        self.event_actions.insert(
            SemanticsAction::SetValue,
            Rc::new(move |ctx, call| {
                if let ActionCall::SetValue(text) = call {
                    handler(ctx, &text);
                }
            }),
        );
        self
    }

    /// Invoke `handler` with the selection a platform asked this field to
    /// take, in bytes of its value: the owner has read it from the field's
    /// published text runs, and refused one whose
    /// ends lie outside them before it reaches here.
    #[must_use]
    pub fn on_set_text_selection<F, R>(mut self, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>, flui_semantics::TextSelection) -> R + 'static,
        R: EventOutcome,
    {
        let handler = value_callback(handler);
        self.event_actions.insert(
            SemanticsAction::SetTextSelection,
            Rc::new(move |ctx, call| {
                if let ActionCall::SetTextSelection(selection) = call {
                    handler(ctx, selection);
                }
            }),
        );
        self
    }

    /// Fold every node beneath this one into it, as a [`MergeSemantics`]
    /// does: for a control's node that describes itself (ADR-0322 §1).
    #[must_use]
    pub(crate) fn merging_descendants(mut self) -> Self {
        self.configuration.set_semantics_boundary(true);
        self.configuration
            .set_merging_semantics_of_descendants(true);
        self
    }

    /// Make this node a merge boundary (ADR-0214 §3): it keeps a node of its
    /// own inside a merging ancestor, and merges none of its descendants into
    /// it, so the controls and texts beneath it stay addressable.
    #[must_use]
    pub fn merge_boundary(mut self, merge_boundary: bool) -> Self {
        self.configuration.set_merge_boundary(merge_boundary);
        self
    }

    /// Publish `selection` as this document's selection across the texts
    /// beneath it, in their runs (ADR-0214 §3). A selection whose end names
    /// no text beneath the node that publishes runs is not published.
    #[must_use]
    pub fn document_selection(
        mut self,
        selection: Option<flui_semantics::DocumentSelection>,
    ) -> Self {
        self.configuration.set_document_selection(selection);
        self
    }

    /// Invoke `handler` with the selection a platform asked this document to
    /// take: a `SetTextSelection` sent to the node, or to a text beneath it,
    /// with each end as the text's anchor and a byte of it (ADR-0214 §4).
    /// The owner refused one whose ends are not current runs of texts beneath
    /// this node before it reaches here. Replaces
    /// [`on_set_text_selection`](Self::on_set_text_selection): a node takes
    /// selections as a field or as a document, not both.
    #[must_use]
    pub fn on_set_document_selection<F, R>(mut self, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>, flui_semantics::DocumentSelection) -> R + 'static,
        R: EventOutcome,
    {
        let handler = value_callback(handler);
        self.event_actions.insert(
            SemanticsAction::SetTextSelection,
            Rc::new(move |ctx, call| {
                if let ActionCall::SetDocumentSelection(selection) = call {
                    handler(ctx, selection);
                }
            }),
        );
        self
    }

    /// Invoke `handler` with `true` when the assistive technology's cursor
    /// (VoiceOver, TalkBack) enters this node's subtree and `false` when it
    /// leaves: a carousel stops moving while a screen reader reads it. This is
    /// not input focus, which the application moves. Only a platform whose
    /// adapter reports the cursor calls it; ask
    /// `LifecycleContext::assistive_focus_support` before relying on it.
    #[must_use]
    pub fn on_assistive_focus_within<F, R>(mut self, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>, bool) -> R + 'static,
        R: EventOutcome,
    {
        self.event_actions.assistive_focus = Some(value_callback(handler));
        self
    }

    /// Invoke `handler` with the text a platform asked this field to put in
    /// place of its selection.
    #[must_use]
    pub fn on_replace_selected_text<F, R>(mut self, handler: F) -> Self
    where
        F: for<'a> Fn(&mut EventContext<'_>, &'a str) -> R + 'static,
        R: EventOutcome,
    {
        let handler = ref_callback::<str, _, _>(handler);
        self.event_actions.insert(
            SemanticsAction::ReplaceSelectedText,
            Rc::new(move |ctx, call| {
                if let ActionCall::ReplaceSelectedText(text) = call {
                    handler(ctx, &text);
                }
            }),
        );
        self
    }

    /// Invoke `handler` with the offset, in this node's coordinates, a
    /// platform asked this node to scroll its content to.
    #[must_use]
    pub fn on_set_scroll_offset<F, R>(mut self, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>, Offset<f64>) -> R + 'static,
        R: EventOutcome,
    {
        let handler = value_callback(handler);
        self.event_actions.insert(
            SemanticsAction::SetScrollOffset,
            Rc::new(move |ctx, call| {
                if let ActionCall::SetScrollOffset(offset) = call {
                    handler(ctx, offset);
                }
            }),
        );
        self
    }

    /// Offer a custom action: an operation assistive technology lists by
    /// `label` (a screen reader's actions menu) beside the node's own, such as
    /// a swipe action or "Move up" in a reorderable list.
    ///
    /// Each call adds one action with its own handler, in order. A label
    /// names one action: a call with a label already offered replaces that
    /// action's handler in place. A fresh closure on every `build` raises no
    /// semantics update; a changed label or count does.
    #[must_use]
    pub fn custom_action<F, R>(self, label: impl Into<String>, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.push_custom_action(label.into(), handler, None)
    }

    /// Offer a custom action only while `enabled` returns `true`.
    ///
    /// Like [`Self::custom_action`], but the platform is shown the action
    /// only when the gate holds, and a request for it while the gate is closed
    /// is refused, whoever sent it (assistive technology or an agent). The
    /// gate is read whenever the node's semantics are described, so an action
    /// whose enablement changes with no rebuild (a page view reaching its last
    /// page) is withdrawn and restored by naming what changes it in
    /// [`Self::refresh_on`].
    #[must_use]
    pub fn custom_action_when<F, R>(
        self,
        label: impl Into<String>,
        enabled: impl Fn() -> bool + 'static,
        handler: F,
    ) -> Self
    where
        F: Fn(&mut EventContext<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.push_custom_action(label.into(), handler, Some(Rc::new(enabled)))
    }

    /// Describe this node's semantics again, without rebuilding anything,
    /// whenever `source` notifies: the signal that a
    /// [`Self::custom_action_when`] gate may now answer differently.
    #[must_use]
    pub fn refresh_on(mut self, source: ChangeNotifier) -> Self {
        self.refresh.push(source);
        self
    }

    fn push_custom_action<F, R>(
        mut self,
        label: String,
        handler: F,
        enabled: Option<Rc<dyn Fn() -> bool>>,
    ) -> Self
    where
        F: Fn(&mut EventContext<'_>) -> R + 'static,
        R: EventOutcome,
    {
        let handler: EventActionHandler =
            Rc::new(move |ctx: &mut EventContext<'_>, _call| handler(ctx).report());
        let entry = CustomEntry {
            label,
            handler,
            enabled,
        };
        // A label names one action of this widget: a second with the same
        // label replaces the first at its place, as a repeated `on_*` does.
        // Two same-labelled entries would be indistinguishable to the user,
        // and their callbacks could trade targets under one published id.
        match self
            .event_actions
            .custom
            .iter_mut()
            .find(|known| known.label == entry.label)
        {
            Some(slot) => *slot = entry,
            None => self.event_actions.custom.push(entry),
        }
        self
    }

    /// Invoke `handler` for `action`, with the call as the platform sent it.
    ///
    /// The escape hatch for what the typed builders do not cover: the text
    /// selection and replacement, tooltip and context-menu actions, whose
    /// handlers read the call's payload themselves. A custom action is added
    /// with its label through [`Self::custom_action`]. A configuration
    /// built by hand and passed to [`Self::from_configuration`] reaches the
    /// same actions with raw `Send + Sync` handlers, but only wholesale; this
    /// adds one action to an annotation assembled by the other builders.
    ///
    /// A fresh closure on every `build` is fine: the node advertises the
    /// action through one handler it keeps across rebuilds, so a rebuild that
    /// changes only the closure raises no semantics update.
    #[must_use]
    pub fn on_action<F, R>(mut self, action: SemanticsAction, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>, ActionCall) -> R + 'static,
        R: EventOutcome,
    {
        self.event_actions.insert(action, value_callback(handler));
        self
    }

    /// Registers a handler for an action that takes no payload.
    fn on_plain_action<F, R>(mut self, action: SemanticsAction, handler: F) -> Self
    where
        F: Fn(&mut EventContext<'_>) -> R + 'static,
        R: EventOutcome,
    {
        // One allocation: the outcome is reported here, as `event_callback`
        // reports it (ADR-0086).
        self.event_actions.insert(
            action,
            Rc::new(move |ctx: &mut EventContext<'_>, _call| handler(ctx).report()),
        );
        self
    }

    /// The configuration to mount: the widget's own, plus every owner-local
    /// action advertised through `route`'s handler.
    fn configuration_with(&self, route: Option<&SemanticsActionRoute>) -> SemanticsConfiguration {
        let mut configuration = self.configuration.clone();
        if let Some(route) = route {
            for action in self.event_actions.actions() {
                configuration.add_action(action, Rc::clone(route.handler()));
            }
            if self.event_actions.assistive_focus.is_some()
                && let Some(handler) = route.assistive_focus_handler()
            {
                configuration.add_assistive_focus_handler(Rc::clone(handler));
            }
        }
        configuration
    }

    /// The custom actions the render object offers beyond the configuration's
    /// own, each advertised through `route`'s handler while its gate holds.
    fn gated_custom_actions(&self, route: Option<&SemanticsActionRoute>) -> Vec<GatedCustomAction> {
        let Some(route) = route else {
            return Vec::new();
        };
        self.event_actions
            .custom
            .iter()
            .map(|entry| {
                GatedCustomAction::new(
                    entry.label.as_str(),
                    Rc::clone(route.handler()),
                    entry.enabled.clone(),
                )
            })
            .collect()
    }

    /// The lane payload for this build's action table, or `None` when there
    /// is none to register or no writer to open its `EventContext` from.
    fn action_cell(&self, ctx: &RenderObjectContext<'_>) -> Option<Rc<dyn Any>> {
        if self.event_actions.is_empty() {
            return None;
        }
        let Some(writer) = ctx.writer_source() else {
            tracing::debug!(
                actions = ?self.event_actions,
                "semantics actions are not advertised: the node is mounted without an owner"
            );
            return None;
        };
        Some(Rc::new(SemanticsActionCell {
            writer,
            actions: self.event_actions.clone(),
            custom_base: self.configuration.custom_actions().count(),
        }))
    }

    /// The handler that tells `target`'s table where the assistive
    /// technology's cursor is.
    fn assistive_focus_route(target: LocalPayloadTarget) -> flui_semantics::AssistiveFocusHandler {
        Rc::new(move |within| deliver_assistive_focus(target, within))
    }

    /// Register this build's action table in the owner lane and mint the
    /// handler that reaches it. The handler for the assistive cursor is
    /// minted only for a table that asks to hear it.
    fn register_actions(&self, ctx: &RenderObjectContext<'_>) -> Option<SemanticsActionRoute> {
        let cell = self.action_cell(ctx)?;
        match ctx.register_local_payload(cell) {
            Ok(target) => {
                let handler: SemanticsActionHandler = Rc::new(move |call| deliver(target, call));
                let route = SemanticsActionRoute::new(target, handler);
                Some(self.with_assistive_focus_route(route))
            }
            Err(error) => {
                tracing::debug!(
                    ?error,
                    "semantics actions are not advertised: no owner lane to hold them"
                );
                None
            }
        }
    }

    /// `route` with the assistive-cursor handler this build needs: minted
    /// once, when a table first asks to hear the cursor, and kept after.
    fn with_assistive_focus_route(&self, route: SemanticsActionRoute) -> SemanticsActionRoute {
        if self.event_actions.assistive_focus.is_none() || route.assistive_focus_handler().is_some()
        {
            return route;
        }
        let target = route.target();
        route.with_assistive_focus(Self::assistive_focus_route(target))
    }

    /// Bring the render object's lane registration in line with this build:
    /// replace the table under the existing ticket (keeping its handler, so
    /// the configuration still compares equal), register one if none exists,
    /// or release it when this build has no actions left.
    fn sync_actions(
        &self,
        ctx: &RenderObjectContext<'_>,
        render_object: &RenderSemanticsAnnotations,
    ) -> Option<SemanticsActionRoute> {
        let Some(existing) = render_object.action_route().cloned() else {
            return self.register_actions(ctx);
        };
        let Some(cell) = self.action_cell(ctx) else {
            release_actions(ctx, &existing);
            return None;
        };
        match ctx.replace_local_payload(existing.target(), cell) {
            Ok(()) => Some(self.with_assistive_focus_route(existing)),
            Err(RenderObjectContextError::Interaction(InteractionDispatchError::TargetGone)) => {
                tracing::debug!("semantics action table was gone; registering anew");
                self.register_actions(ctx)
            }
            Err(error) => {
                // The lane cannot be reached from this update, so it could
                // neither register a new table nor release the old one. Keep
                // the existing route: its ticket stays the one unmount
                // releases, rather than orphaning it in the lane.
                tracing::debug!(
                    ?error,
                    "semantics action table not replaced: the owner lane is unreachable"
                );
                Some(existing)
            }
        }
    }

    /// The route this build advertises its actions through: kept in line
    /// with the build while the presentation collects semantics, released
    /// while it does not.
    fn route_while_collected(
        &self,
        ctx: &RenderObjectContext<'_>,
        render_object: &RenderSemanticsAnnotations,
    ) -> Option<SemanticsActionRoute> {
        if ctx.collects_semantics() {
            return self.sync_actions(ctx, render_object);
        }
        if let Some(existing) = render_object.action_route() {
            release_actions(ctx, existing);
        }
        None
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

/// Release the action table `part` holds in the owner lane: what a
/// [`Semantics`] element does when it unmounts, for any owner of a
/// [`RenderSemanticsAnnotations`].
pub(crate) fn release_part(ctx: &RenderObjectContext<'_>, part: &mut RenderSemanticsAnnotations) {
    if let Some(route) = part.set_action_route(None) {
        release_actions(ctx, &route);
    }
}

/// Remove a node's action table from the owner lane.
fn release_actions(ctx: &RenderObjectContext<'_>, route: &SemanticsActionRoute) {
    if let Err(error) = ctx.unregister_local_payload(route.target()) {
        tracing::debug!(?error, "semantics action table was already unregistered");
    }
}

impl RenderView for Semantics {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSemanticsAnnotations;

    fn create_render_object(&self, ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        let route = ctx
            .collects_semantics()
            .then(|| self.register_actions(ctx))
            .flatten();
        let mut render_object =
            RenderSemanticsAnnotations::from_configuration(self.configuration_with(route.as_ref()))
                .with_container(self.options.contains(SemanticsOptions::CONTAINER))
                .with_explicit_child_nodes(
                    self.options
                        .contains(SemanticsOptions::EXPLICIT_CHILD_NODES),
                )
                .with_exclude_semantics(
                    self.options.contains(SemanticsOptions::EXCLUDE_DESCENDANTS),
                )
                .with_block_user_actions(
                    self.options.contains(SemanticsOptions::BLOCK_USER_ACTIONS),
                );
        let _impact = render_object.set_scroll_source(self.scroll.clone());
        let _impact = render_object.set_custom_actions(
            self.configuration.custom_actions().count(),
            self.gated_custom_actions(route.as_ref()),
        );
        render_object.set_refresh_sources(self.refresh.clone());
        let _none = render_object.set_action_route(route);
        render_object
    }

    fn update_render_object(
        &self,
        ctx: &RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let route = self.route_while_collected(ctx, render_object);
        let configuration = self.configuration_with(route.as_ref());
        let custom = self.gated_custom_actions(route.as_ref());
        let _previous = render_object.set_action_route(route);
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_configuration(configuration);
        impact |=
            render_object.set_custom_actions(self.configuration.custom_actions().count(), custom);
        render_object.set_refresh_sources(self.refresh.clone());
        impact |= render_object.set_scroll_source(self.scroll.clone());
        impact |= render_object.set_container(self.options.contains(SemanticsOptions::CONTAINER));
        impact |= render_object.set_explicit_child_nodes(
            self.options
                .contains(SemanticsOptions::EXPLICIT_CHILD_NODES),
        );
        impact |= render_object
            .set_exclude_semantics(self.options.contains(SemanticsOptions::EXCLUDE_DESCENDANTS));
        impact |= render_object
            .set_block_user_actions(self.options.contains(SemanticsOptions::BLOCK_USER_ACTIONS));
        impact
    }

    fn did_unmount_render_object(
        &self,
        ctx: &RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        release_part(ctx, render_object);
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(Semantics, site = site);
