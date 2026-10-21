//! Accessibility semantics widgets.
//!
//! These widgets are thin `RenderView` wrappers over the semantics proxy render
//! objects in `flui-objects`, split into `Semantics`, `MergeSemantics`, and
//! `ExcludeSemantics`.
//!
//! # Action handlers are owner-local and receive an `EventCx`
//!
//! [`Semantics`]'s `on_*` builders register handlers that assistive technology
//! can invoke, and each receives the `&mut EventCx<'_>` a write through the
//! widget's `WriterSource` opens (ADR-0086), exactly like a pointer callback:
//! "activation toggles this control's own state" is a signal write.
//!
//! The configuration a semantics node carries still stores
//! `flui_semantics::SemanticsActionHandler`, an `Arc<dyn Fn(..) + Send + Sync>`,
//! because it rides in the semantics proxy render object, which
//! `flui_view::RenderView` pins `Send + Sync`. So the widget's closures do not
//! go there. They stay in the owner's interaction lane as one table per node,
//! and the configuration advertises each action through one `Send + Sync`
//! handler that holds only the lane's ticket. Invoking it resolves the ticket
//! on the owner thread, inside the UI runtime, and runs the closure there.
//!
//! An action invoked outside its UI runtime — no interaction lane active on the
//! calling thread — has no owner to run in and is dropped with a warning. A
//! [`Semantics`] mounted without an owner lane (a detached render-object
//! context) advertises none of these actions, so no platform sees a control
//! that nothing can run. [`Semantics::from_configuration`] still takes raw
//! `Send + Sync` handlers; that
//! is the `flui-semantics` surface, below this widget. `ARCHITECTURE.md` §17
//! records the design.

use std::any::Any;
use std::num::{NonZeroU32, NonZeroUsize};
use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::geometry::Offset;
use flui_interaction::{InteractionDispatchError, LocalPayloadTarget, resolve_local_payload};
use flui_objects::{
    RenderExcludeSemantics, RenderIndexedSemantics, RenderMergeSemantics,
    RenderSemanticsAnnotations, RenderSliverSemanticsAnnotations, SemanticsActionRoute,
};
use flui_rendering::{
    protocol::{BoxProtocol, SliverProtocol},
    semantics::{
        ActionCall, Assertiveness, CellPosition, HeadingLevel, Reveal, ScrollUnit, SemanticsAction,
        SemanticsActionHandler, SemanticsAnchor, SemanticsConfiguration, SemanticsRelation,
        SemanticsRole, SetSize, SortDirection, TableShape, TextDirection, Toggled,
    },
};
use flui_view::{
    Child, EventCx, EventOutcome, IntoView, RenderObjectContext, RenderObjectContextError,
    RenderView, WriterSource, impl_render_view,
};

use crate::support::{event_callback, ref_callback, value_callback};

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

/// One owner-local action handler: the dispatch's `EventCx` and the call,
/// payload included.
type EventActionHandler = Rc<dyn Fn(&mut EventCx<'_>, ActionCall)>;

/// The owner-local action handlers one [`Semantics`] registered, in
/// registration order: one per action (a later registration replaces an
/// earlier one, as `SemanticsConfiguration::add_action` does), and any number
/// of labelled custom actions.
#[derive(Clone, Default)]
struct EventActions {
    handlers: Vec<(SemanticsAction, EventActionHandler)>,
    custom: Vec<(String, EventActionHandler)>,
}

impl EventActions {
    fn insert(&mut self, action: SemanticsAction, handler: EventActionHandler) {
        match self.handlers.iter_mut().find(|(known, _)| *known == action) {
            Some((_, slot)) => *slot = handler,
            None => self.handlers.push((action, handler)),
        }
    }

    fn get(&self, action: SemanticsAction) -> Option<EventActionHandler> {
        self.handlers
            .iter()
            .find(|(known, _)| *known == action)
            .map(|(_, handler)| Rc::clone(handler))
    }

    fn is_empty(&self) -> bool {
        self.handlers.is_empty() && self.custom.is_empty()
    }

    fn actions(&self) -> impl Iterator<Item = SemanticsAction> + '_ {
        self.handlers.iter().map(|(action, _)| *action)
    }
}

impl std::fmt::Debug for EventActions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventActions")
            .field("handlers", &self.actions().collect::<Vec<_>>())
            .field(
                "custom",
                &self
                    .custom
                    .iter()
                    .map(|(label, _)| label)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// What a mounted [`Semantics`] keeps in the owner's interaction lane: its
/// action table, the writer each handler's `EventCx` is opened from, and how
/// many custom actions the configuration declared before the table's own, so
/// a custom action's id finds its handler.
struct SemanticsActionCell {
    writer: WriterSource,
    actions: EventActions,
    custom_base: usize,
}

/// Run `action`'s owner-local handler for the node whose table `target`
/// names.
///
/// Reached through the `Send + Sync` handler the configuration advertises,
/// which the semantics owner invokes on the owner thread while the UI runtime is
/// entered. Every failure is a dropped action, never a panic: the platform
/// asked for something the tree can no longer do.
fn deliver(target: LocalPayloadTarget, call: ActionCall) {
    let action = call.action();
    let payload = match resolve_local_payload(target) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(
                ?error,
                ?action,
                "a semantics action was dropped: its handler is owner-local and runs only \
                 inside its ui_runtime"
            );
            return;
        }
    };
    let Ok(cell) = payload.downcast::<SemanticsActionCell>() else {
        tracing::error!(
            ?action,
            "BUG: a semantics action ticket resolved to a payload of another type"
        );
        return;
    };
    let handler = if let ActionCall::CustomAction(id) = &call {
        // The id is the one the node's configuration gave the action; the
        // widget's own follow any the configuration declared before them.
        id.index()
            .checked_sub(cell.custom_base)
            .and_then(|index| cell.actions.custom.get(index))
            .map(|(_, handler)| Rc::clone(handler))
    } else {
        cell.actions.get(action)
    };
    let Some(handler) = handler else {
        tracing::debug!(
            ?action,
            "a semantics action arrived after its handler was removed; dropped"
        );
        return;
    };
    cell.writer.write(|cx| handler(cx, call));
}

/// Annotates a subtree with accessibility semantics.
#[derive(Clone)]
pub struct Semantics {
    configuration: SemanticsConfiguration,
    event_actions: EventActions,
    options: SemanticsOptions,
    scroll: Option<(
        flui_rendering::view::ScrollPosition,
        flui_foundation::geometry::Axis,
        bool,
    )>,
    child: Child,
}

impl std::fmt::Debug for Semantics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Semantics")
            .field("configuration", &self.configuration)
            .field("scroll", &self.scroll)
            .field("event_actions", &self.event_actions)
            .field("options", &self.options)
            .field("child", &self.child)
            .finish()
    }
}

impl Default for Semantics {
    fn default() -> Self {
        Self {
            configuration: SemanticsConfiguration::new(),
            event_actions: EventActions::default(),
            options: SemanticsOptions::default(),
            scroll: None,
            child: Child::empty(),
        }
    }
}

impl Semantics {
    /// Creates an empty semantics annotation.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a semantics annotation from a ready configuration.
    pub fn from_configuration(configuration: SemanticsConfiguration) -> Self {
        Self {
            configuration,
            ..Self::default()
        }
    }

    pub(crate) fn scroll_source(
        mut self,
        position: flui_rendering::view::ScrollPosition,
        axis: flui_foundation::geometry::Axis,
        reversed: bool,
    ) -> Self {
        self.scroll = Some((position, axis, reversed));
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
    pub fn label(mut self, label: impl Into<flui_rendering::semantics::AttributedString>) -> Self {
        self.configuration.set_label(label);
        self
    }

    /// Set the stable identifier tests and automation select this node by
    /// (ADR-0240), exported as UI Automation's `AutomationId`, macOS
    /// `AXIdentifier` and AT-SPI's accessible id. It is never announced, and
    /// it does not make a node: it names this widget's node only when the
    /// widget forms one ([`container`](Self::container), or under a parent
    /// that requires explicit child nodes). Absorbed into an ancestor or
    /// merged by `MergeSemantics`, it is dropped, not carried up.
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
    pub fn value(mut self, value: impl Into<flui_rendering::semantics::AttributedString>) -> Self {
        self.configuration.set_value(value);
        self
    }

    /// Set the accessible hint.
    #[must_use]
    pub fn hint(mut self, hint: impl Into<flui_rendering::semantics::AttributedString>) -> Self {
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

    /// Set whether this form control must be filled before submitting.
    #[must_use]
    pub fn required(mut self, required: bool) -> Self {
        self.configuration.set_required(required);
        self
    }

    /// Set whether this form control's value failed validation.
    #[must_use]
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.configuration.set_invalid(invalid);
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
    #[must_use]
    pub fn live_region(mut self, politeness: Assertiveness) -> Self {
        self.configuration.set_live_region(Some(politeness));
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
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_plain_action(SemanticsAction::Click, handler)
    }

    /// Invoke `handler` when assistive technology asks for this node's context
    /// menu.
    #[must_use]
    pub fn on_show_context_menu<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_plain_action(SemanticsAction::ShowContextMenu, handler)
    }

    /// Invoke `handler` when assistive technology scrolls this node left, by
    /// the [`ScrollUnit`] it asked for.
    #[must_use]
    pub fn on_scroll_left<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, ScrollUnit) -> R + 'static,
        R: EventOutcome,
    {
        self.on_scroll_action(SemanticsAction::ScrollLeft, handler)
    }

    /// Invoke `handler` when assistive technology scrolls this node right, by
    /// the [`ScrollUnit`] it asked for.
    #[must_use]
    pub fn on_scroll_right<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, ScrollUnit) -> R + 'static,
        R: EventOutcome,
    {
        self.on_scroll_action(SemanticsAction::ScrollRight, handler)
    }

    /// Invoke `handler` when assistive technology scrolls this node up, by the
    /// [`ScrollUnit`] it asked for.
    #[must_use]
    pub fn on_scroll_up<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, ScrollUnit) -> R + 'static,
        R: EventOutcome,
    {
        self.on_scroll_action(SemanticsAction::ScrollUp, handler)
    }

    /// Invoke `handler` when assistive technology scrolls this node down, by
    /// the [`ScrollUnit`] it asked for.
    #[must_use]
    pub fn on_scroll_down<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, ScrollUnit) -> R + 'static,
        R: EventOutcome,
    {
        self.on_scroll_action(SemanticsAction::ScrollDown, handler)
    }

    /// Registers a directional scroll handler, which receives the call's unit.
    fn on_scroll_action<F, R>(mut self, action: SemanticsAction, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, ScrollUnit) -> R + 'static,
        R: EventOutcome,
    {
        let handler = value_callback(handler);
        self.event_actions.insert(
            action,
            Rc::new(move |cx, call| match call {
                ActionCall::ScrollUp(unit)
                | ActionCall::ScrollDown(unit)
                | ActionCall::ScrollLeft(unit)
                | ActionCall::ScrollRight(unit) => handler(cx, unit),
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
    pub fn numeric_range(mut self, range: flui_rendering::semantics::NumericRange) -> Self {
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
    /// let _ = flui_widgets::Semantics::new().on_expand(|_cx| {});
    /// ```
    ///
    /// Register the expanded state and both transition handlers together:
    ///
    /// ```
    /// let _ = flui_widgets::Semantics::new().expandable(false, |_cx| {}, |_cx| {});
    /// ```
    #[must_use]
    pub fn expandable<E, RE, C, RC>(self, expanded: bool, on_expand: E, on_collapse: C) -> Self
    where
        E: Fn(&mut EventCx<'_>) -> RE + 'static,
        RE: EventOutcome,
        C: Fn(&mut EventCx<'_>) -> RC + 'static,
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
        F: Fn(&mut EventCx<'_>, f64) -> R + 'static,
        R: EventOutcome,
    {
        let handler = value_callback(handler);
        self.event_actions.insert(
            SemanticsAction::SetNumericValue,
            Rc::new(move |cx, call| {
                if let ActionCall::SetNumericValue(value) = call {
                    handler(cx, value);
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
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_plain_action(SemanticsAction::Increment, handler)
    }

    /// Invoke `handler` when assistive technology lowers this node's value
    /// one step.
    #[must_use]
    pub fn on_decrement<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
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
        F: Fn(&mut EventCx<'_>, Option<Reveal>) -> R + 'static,
        R: EventOutcome,
    {
        let handler = value_callback(handler);
        self.event_actions.insert(
            SemanticsAction::ScrollIntoView,
            Rc::new(move |cx, call| {
                if let ActionCall::ScrollIntoView(reveal) = call {
                    handler(cx, reveal);
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
        F: Fn(&mut EventCx<'_>) -> R + 'static,
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
        F: Fn(&mut EventCx<'_>) -> R + 'static,
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
        F: for<'a> Fn(&mut EventCx<'_>, &'a str) -> R + 'static,
        R: EventOutcome,
    {
        let handler = ref_callback::<str, _, _>(handler);
        self.event_actions.insert(
            SemanticsAction::SetValue,
            Rc::new(move |cx, call| {
                if let ActionCall::SetValue(text) = call {
                    handler(cx, &text);
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
        F: Fn(&mut EventCx<'_>, Offset<f64>) -> R + 'static,
        R: EventOutcome,
    {
        let handler = value_callback(handler);
        self.event_actions.insert(
            SemanticsAction::SetScrollOffset,
            Rc::new(move |cx, call| {
                if let ActionCall::SetScrollOffset(offset) = call {
                    handler(cx, offset);
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
    pub fn custom_action<F, R>(mut self, label: impl Into<String>, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        let handler = event_callback(handler);
        let handler: EventActionHandler = Rc::new(move |cx, _call| handler(cx));
        let label = label.into();
        // A label names one action of this widget: a second with the same
        // label replaces the first at its place, as a repeated `on_*` does.
        // Two same-labelled entries would be indistinguishable to the user,
        // and their callbacks could trade targets under one published id.
        match self
            .event_actions
            .custom
            .iter_mut()
            .find(|(known, _)| *known == label)
        {
            Some((_, slot)) => *slot = handler,
            None => self.event_actions.custom.push((label, handler)),
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
        F: Fn(&mut EventCx<'_>, ActionCall) -> R + 'static,
        R: EventOutcome,
    {
        self.event_actions.insert(action, value_callback(handler));
        self
    }

    /// Registers a handler for an action that takes no payload.
    fn on_plain_action<F, R>(mut self, action: SemanticsAction, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        let handler = event_callback(handler);
        self.event_actions
            .insert(action, Rc::new(move |cx, _call| handler(cx)));
        self
    }

    /// The configuration to mount: the widget's own, plus every owner-local
    /// action advertised through `route`'s handler.
    fn configuration_with(&self, route: Option<&SemanticsActionRoute>) -> SemanticsConfiguration {
        let mut configuration = self.configuration.clone();
        if let Some(route) = route {
            for (label, _) in &self.event_actions.custom {
                configuration.add_custom_action(label.as_str(), Arc::clone(route.handler()));
            }
            for action in self.event_actions.actions() {
                configuration.add_action(action, Arc::clone(route.handler()));
            }
        }
        configuration
    }

    /// The lane payload for this build's action table, or `None` when there
    /// is none to register or no writer to open its `EventCx` from.
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

    /// Register this build's action table in the owner lane and mint the
    /// handler that reaches it.
    fn register_actions(&self, ctx: &RenderObjectContext<'_>) -> Option<SemanticsActionRoute> {
        let cell = self.action_cell(ctx)?;
        match ctx.register_local_payload(cell) {
            Ok(target) => {
                let handler: SemanticsActionHandler = Arc::new(move |call| deliver(target, call));
                Some(SemanticsActionRoute::new(target, handler))
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
            Ok(()) => Some(existing),
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

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
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
        let route = self.register_actions(ctx);
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
        let _none = render_object.set_action_route(route);
        render_object
    }

    fn update_render_object(
        &self,
        ctx: &RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let route = self.sync_actions(ctx, render_object);
        let configuration = self.configuration_with(route.as_ref());
        let _previous = render_object.set_action_route(route);
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_configuration(configuration);
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
        if let Some(route) = render_object.set_action_route(None) {
            release_actions(ctx, &route);
        }
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(Semantics);

/// Annotates an entire sliver subtree while retaining the sliver protocol.
/// Use [`Semantics`] for individual box children. A supplied configuration may
/// carry ordinary typed actions, collection metadata, relations or sort keys.
#[derive(Clone, Debug, Default)]
pub struct SliverSemantics {
    configuration: SemanticsConfiguration,
    exclude_descendants: bool,
    child: Child,
}

impl SliverSemantics {
    /// Creates an empty sliver annotation.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an annotation from a complete configuration.
    pub fn from_configuration(configuration: SemanticsConfiguration) -> Self {
        Self {
            configuration,
            ..Self::default()
        }
    }

    /// Supplies the accessible label.
    #[must_use]
    pub fn label(mut self, label: impl Into<flui_rendering::semantics::AttributedString>) -> Self {
        self.configuration.set_label(label);
        self
    }

    /// Whether this sliver forms a semantics boundary.
    #[must_use]
    pub fn container(mut self, container: bool) -> Self {
        self.configuration.set_semantics_boundary(container);
        self
    }

    /// Keeps descendant contributions in explicit accessible nodes.
    #[must_use]
    pub fn explicit_child_nodes(mut self, explicit: bool) -> Self {
        self.configuration.set_explicit_child_nodes(explicit);
        self
    }

    /// Drops descendant accessibility without changing layout or painting.
    #[must_use]
    pub fn exclude_semantics(mut self, exclude: bool) -> Self {
        self.exclude_descendants = exclude;
        self
    }

    /// Supplies the sliver child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for SliverSemantics {
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverSemanticsAnnotations;

    fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        let mut object =
            RenderSliverSemanticsAnnotations::from_configuration(self.configuration.clone());
        let _ = object.set_exclude_descendants(self.exclude_descendants);
        object
    }

    fn update_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        object.set_configuration(self.configuration.clone())
            | object.set_exclude_descendants(self.exclude_descendants)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(SliverSemantics);

/// Merges the semantics of all descendants into a single node.
#[derive(Clone, Debug, Default)]
pub struct MergeSemantics {
    identifier: Option<std::sync::Arc<str>>,
    child: Child,
}

impl MergeSemantics {
    /// Creates a merge-semantics widget.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the merged node's stable identifier for tests and automation
    /// (ADR-0240). The merge forms the node, so this, not a descendant's
    /// [`Semantics::identifier`], names it: a descendant's identifier is
    /// dropped by the merge.
    #[must_use]
    pub fn identifier(mut self, identifier: impl Into<std::sync::Arc<str>>) -> Self {
        self.identifier = Some(identifier.into());
        self
    }

    /// [`identifier`](Self::identifier) when `identifier` is `Some`.
    #[must_use]
    pub(crate) fn identifier_if(mut self, identifier: Option<&std::sync::Arc<str>>) -> Self {
        self.identifier = identifier.cloned();
        self
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for MergeSemantics {
    type Protocol = BoxProtocol;
    type RenderObject = RenderMergeSemantics;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        let mut object = RenderMergeSemantics::default();
        let _ = object.set_identifier(self.identifier.clone());
        object
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_identifier(self.identifier.clone())
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(MergeSemantics);

/// Annotates its child's semantics node with a zero-based index among its
/// siblings.
///
/// This is the "12" a screen reader announces in "item 12 of 100"; the "100"
/// comes from the enclosing scrollable's own child count, not from here.
///
/// Lazy sliver delegates do not wrap their materialised items in one — see
/// flui-rendering's `## Mapping decisions` — so this is for content you
/// index yourself: a hand-built list, a grid of cards, anything a lazy sliver
/// does not own.
///
/// The index is zero-based, as AccessKit's `position_in_set` is; the platform
/// adapters add one when they hand it to a screen reader.
#[derive(Clone, Debug, Default)]
pub struct IndexedSemantics {
    index: usize,
    child: Child,
}

impl IndexedSemantics {
    /// Creates an indexed-semantics widget for a zero-based `index`.
    #[must_use]
    pub fn new(index: usize) -> Self {
        Self {
            index,
            child: Child::empty(),
        }
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for IndexedSemantics {
    type Protocol = BoxProtocol;
    type RenderObject = RenderIndexedSemantics;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderIndexedSemantics::new(self.index)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_index(self.index)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(IndexedSemantics);

/// Drops descendant semantics while keeping layout, paint, and hit testing.
#[derive(Clone, Debug)]
pub struct ExcludeSemantics {
    excluding: bool,
    child: Child,
}

impl Default for ExcludeSemantics {
    fn default() -> Self {
        Self {
            excluding: true,
            child: Child::empty(),
        }
    }
}

impl ExcludeSemantics {
    /// Creates an exclude-semantics widget.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether descendants are excluded from semantics.
    #[must_use]
    pub fn excluding(mut self, excluding: bool) -> Self {
        self.excluding = excluding;
        self
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for ExcludeSemantics {
    type Protocol = BoxProtocol;
    type RenderObject = RenderExcludeSemantics;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderExcludeSemantics::new(self.excluding)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_excluding(self.excluding);
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(ExcludeSemantics);
