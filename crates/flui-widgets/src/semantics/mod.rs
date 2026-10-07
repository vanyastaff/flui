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
//! on the owner thread, inside the realm, and runs the closure there.
//!
//! An action invoked outside its realm — no interaction lane active on the
//! calling thread — has no owner to run in and is dropped with a warning. A
//! [`Semantics`] mounted without an owner lane (a detached render-object
//! context) advertises none of these actions, so no platform sees a control
//! that nothing can run. [`Semantics::from_configuration`] and
//! [`Semantics::from_properties`] still take raw `Send + Sync` handlers; that
//! is the `flui-semantics` surface, below this widget. `ARCHITECTURE.md` §17
//! records the design.

use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;

use flui_interaction::{InteractionDispatchError, LocalPayloadTarget, resolve_local_payload};
use flui_objects::{
    RenderExcludeSemantics, RenderIndexedSemantics, RenderMergeSemantics,
    RenderSemanticsAnnotations, SemanticsActionRoute,
};
use flui_rendering::{
    protocol::BoxProtocol,
    semantics::{
        ActionArgs, SemanticsAction, SemanticsActionHandler, SemanticsConfiguration,
        SemanticsProperties, SemanticsRole, TextDirection,
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

/// One owner-local action handler: the dispatch's `EventCx` and the action's
/// optional payload.
type EventActionHandler = Rc<dyn Fn(&mut EventCx<'_>, Option<ActionArgs>)>;

/// The owner-local action handlers one [`Semantics`] registered, in
/// registration order, one per action (a later registration replaces an
/// earlier one, as `SemanticsConfiguration::add_action` does).
#[derive(Clone, Default)]
struct EventActions {
    handlers: Vec<(SemanticsAction, EventActionHandler)>,
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
        self.handlers.is_empty()
    }

    fn actions(&self) -> impl Iterator<Item = SemanticsAction> + '_ {
        self.handlers.iter().map(|(action, _)| *action)
    }
}

impl std::fmt::Debug for EventActions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.actions()).finish()
    }
}

/// What a mounted [`Semantics`] keeps in the owner's interaction lane: its
/// action table and the writer each handler's `EventCx` is opened from.
struct SemanticsActionCell {
    writer: WriterSource,
    actions: EventActions,
}

/// Run `action`'s owner-local handler for the node whose table `target`
/// names.
///
/// Reached through the `Send + Sync` handler the configuration advertises,
/// which the semantics owner invokes on the owner thread while the realm is
/// entered. Every failure is a dropped action, never a panic: the platform
/// asked for something the tree can no longer do.
fn deliver(target: LocalPayloadTarget, action: SemanticsAction, arguments: Option<ActionArgs>) {
    let payload = match resolve_local_payload(target) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(
                ?error,
                ?action,
                "a semantics action was dropped: its handler is owner-local and runs only \
                 inside its realm"
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
    let Some(handler) = cell.actions.get(action) else {
        tracing::debug!(
            ?action,
            "a semantics action arrived after its handler was removed; dropped"
        );
        return;
    };
    cell.writer.write(|cx| handler(cx, arguments));
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

    /// Creates a semantics annotation from the shared properties bag.
    pub fn from_properties(properties: &SemanticsProperties) -> Self {
        Self {
            configuration: SemanticsConfiguration::from_properties(properties),
            ..Self::default()
        }
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

    /// Set the accessible label.
    #[must_use]
    pub fn label(mut self, label: impl Into<flui_rendering::semantics::AttributedString>) -> Self {
        self.configuration.set_label(label);
        self
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

    /// Set whether this node has checked state and is checked.
    #[must_use]
    pub fn checked(mut self, checked: bool) -> Self {
        self.configuration.set_checked(Some(checked));
        self
    }

    /// Set whether this node belongs to a mutually-exclusive group.
    ///
    /// On a checkable node this flag is what distinguishes a radio button
    /// from a checkbox: the platform translation resolves checked state
    /// inside a mutually-exclusive group to a radio-button role, and checked
    /// state without it to a checkbox role.
    #[must_use]
    pub fn in_mutually_exclusive_group(mut self, in_mutually_exclusive_group: bool) -> Self {
        self.configuration
            .set_in_mutually_exclusive_group(in_mutually_exclusive_group);
        self
    }

    /// Set whether this node is in a mixed checkbox state.
    #[must_use]
    pub fn mixed(mut self, mixed: bool) -> Self {
        self.configuration.set_mixed(mixed);
        self
    }

    /// Set whether this node has toggled state and is toggled.
    #[must_use]
    pub fn toggled(mut self, toggled: bool) -> Self {
        self.configuration.set_toggled(Some(toggled));
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

    /// Set whether this node is a button.
    #[must_use]
    pub fn button(mut self, button: bool) -> Self {
        self.configuration.set_button(button);
        self
    }

    /// Set whether this node is a link.
    #[must_use]
    pub fn link(mut self, link: bool) -> Self {
        self.configuration.set_link(link);
        self
    }

    /// Set whether this node is a slider.
    #[must_use]
    pub fn slider(mut self, slider: bool) -> Self {
        self.configuration.set_slider(slider);
        self
    }

    /// Set whether this node is a header.
    #[must_use]
    pub fn header(mut self, header: bool) -> Self {
        self.configuration.set_header(header);
        self
    }

    /// Set whether this node is an image.
    #[must_use]
    pub fn image(mut self, image: bool) -> Self {
        self.configuration.set_image(image);
        self
    }

    /// Set whether this node is a text field.
    #[must_use]
    pub fn text_field(mut self, text_field: bool) -> Self {
        self.configuration.set_text_field(text_field);
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

    /// Set whether this node is obscured, such as a password field.
    #[must_use]
    pub fn obscured(mut self, obscured: bool) -> Self {
        self.configuration.set_obscured(obscured);
        self
    }

    /// Set whether this node is multiline.
    #[must_use]
    pub fn multiline(mut self, multiline: bool) -> Self {
        self.configuration.set_multiline(multiline);
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

    /// Set whether this node is a live region.
    #[must_use]
    pub fn live_region(mut self, live_region: bool) -> Self {
        self.configuration.set_live_region(live_region);
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
    pub fn on_tap<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_plain_action(SemanticsAction::Tap, handler)
    }

    /// Invoke `handler` when assistive technology requests a context menu.
    #[must_use]
    pub fn on_long_press<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_plain_action(SemanticsAction::LongPress, handler)
    }

    /// Invoke `handler` when assistive technology scrolls this node left.
    #[must_use]
    pub fn on_scroll_left<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_plain_action(SemanticsAction::ScrollLeft, handler)
    }

    /// Invoke `handler` when assistive technology scrolls this node right.
    #[must_use]
    pub fn on_scroll_right<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_plain_action(SemanticsAction::ScrollRight, handler)
    }

    /// Invoke `handler` when assistive technology scrolls this node up.
    #[must_use]
    pub fn on_scroll_up<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_plain_action(SemanticsAction::ScrollUp, handler)
    }

    /// Invoke `handler` when assistive technology scrolls this node down.
    #[must_use]
    pub fn on_scroll_down<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_plain_action(SemanticsAction::ScrollDown, handler)
    }

    /// Publishes a validated numeric range for native range-value controls.
    ///
    /// This always marks the node as a slider, as [`Self::slider`]`(true)`
    /// does, so it publishes the slider role unless [`Self::role`] or a more
    /// specific role flag (a checked or toggled state, button, link or text
    /// field) takes precedence.
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
    /// For a control that toggles through its tap handler alone, publish the
    /// state with [`Self::expanded`] and register [`Self::on_tap`]: that
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
            Rc::new(move |cx, args| {
                if let Some(ActionArgs::SetNumericValue { value }) = args {
                    handler(cx, value);
                }
            }),
        );
        self
    }

    /// Invoke `handler` when assistive technology increments this node's value.
    ///
    /// The one-step-up counterpart to [`Self::on_decrease`], for sliders and
    /// steppers whose value a screen reader can adjust without a pointer.
    #[must_use]
    pub fn on_increase<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_plain_action(SemanticsAction::Increase, handler)
    }

    /// Invoke `handler` when assistive technology decrements this node's value.
    #[must_use]
    pub fn on_decrease<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_plain_action(SemanticsAction::Decrease, handler)
    }

    /// Invoke `handler` when assistive technology asks for this node to be
    /// brought into view.
    ///
    /// How a screen reader moves a scrollable to whatever it is reading
    /// *next*, which is not always something the pointer path can trigger —
    /// the platform asks for a node that is currently offscreen.
    #[must_use]
    pub fn on_show_on_screen<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_plain_action(SemanticsAction::ShowOnScreen, handler)
    }

    /// Invoke `handler` when assistive technology moves focus to this node.
    #[must_use]
    pub fn on_focus<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_plain_action(SemanticsAction::Focus, handler)
    }

    /// Invoke `handler` when assistive technology moves focus away from this node.
    ///
    /// The notification counterpart to [`Self::on_focus`], and deliberately not
    /// its mirror image: the platform reports losing focus as a notification
    /// about something that already happened, whereas a focus request is a
    /// command the node may refuse.
    #[must_use]
    pub fn on_blur<F, R>(self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_plain_action(SemanticsAction::DidLoseAccessibilityFocus, handler)
    }

    /// Invoke `handler` with the text a platform asked this node to hold.
    ///
    /// How a screen reader enters text into a custom-drawn field. A request
    /// that arrives without a text payload is dropped with a trace rather than
    /// passed on as an empty string — `""` is a legitimate edit a platform
    /// could mean, so synthesizing one would turn a lost payload into a silent
    /// erasure of the field's contents.
    #[must_use]
    pub fn on_set_text<F, R>(mut self, handler: F) -> Self
    where
        F: for<'a> Fn(&mut EventCx<'_>, &'a str) -> R + 'static,
        R: EventOutcome,
    {
        let handler = ref_callback::<str, _, _>(handler);
        self.event_actions.insert(
            SemanticsAction::SetText,
            Rc::new(move |cx, args| {
                if let Some(ActionArgs::SetText { text }) = args {
                    handler(cx, &text);
                } else {
                    tracing::warn!(
                        "dropping a set-text request whose payload did not cross the translation \
                         seam; the node's existing content is left unchanged"
                    );
                }
            }),
        );
        self
    }

    /// Invoke `handler` with the offset a platform asked this node to scroll to.
    ///
    /// As with [`Self::on_set_text`], a request that arrives without an offset
    /// payload is dropped with a trace rather than passed on as `(0.0, 0.0)` —
    /// the origin is a position a platform can legitimately mean, so inventing
    /// it would scroll the view somewhere the request never asked for.
    #[must_use]
    pub fn on_scroll_to_offset<F, R>(mut self, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, f64, f64) -> R + 'static,
        R: EventOutcome,
    {
        let handler =
            value_callback(move |cx: &mut EventCx<'_>, (x, y): (f64, f64)| handler(cx, x, y));
        self.event_actions.insert(
            SemanticsAction::ScrollToOffset,
            Rc::new(move |cx, args| {
                if let Some(ActionArgs::ScrollToOffset { x, y }) = args {
                    handler(cx, (x, y));
                } else {
                    tracing::warn!(
                        "dropping a scroll-to-offset request whose payload did not cross the \
                         translation seam; the scroll position is left unchanged"
                    );
                }
            }),
        );
        self
    }

    /// Invoke `handler` for `action`, with the action's payload as the
    /// platform sent it.
    ///
    /// The escape hatch for what the typed builders deliberately do not cover
    /// — actions the platform vocabulary cannot route yet, and payloads a
    /// typed builder would narrow. A configuration built by hand and passed to
    /// [`Self::from_configuration`] reaches the same actions with raw
    /// `Send + Sync` handlers, but only wholesale; this adds one action to an
    /// annotation assembled by the other builders.
    ///
    /// A fresh closure on every `build` is fine: the node advertises the
    /// action through one handler it keeps across rebuilds, so a rebuild that
    /// changes only the closure raises no semantics update.
    #[must_use]
    pub fn on_action<F, R>(mut self, action: SemanticsAction, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, Option<ActionArgs>) -> R + 'static,
        R: EventOutcome,
    {
        self.event_actions.insert(action, value_callback(handler));
        self
    }

    /// Registers a no-argument action handler. The action's payload is
    /// dropped: every action routed through here takes none, and the typed
    /// builder already knows which action it is bound to.
    fn on_plain_action<F, R>(mut self, action: SemanticsAction, handler: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        let handler = event_callback(handler);
        self.event_actions
            .insert(action, Rc::new(move |cx, _args| handler(cx)));
        self
    }

    /// The configuration to mount: the widget's own, plus every owner-local
    /// action advertised through `route`'s handler.
    fn configuration_with(&self, route: Option<&SemanticsActionRoute>) -> SemanticsConfiguration {
        let mut configuration = self.configuration.clone();
        if let Some(route) = route {
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
        }))
    }

    /// Register this build's action table in the owner lane and mint the
    /// handler that reaches it.
    fn register_actions(&self, ctx: &RenderObjectContext<'_>) -> Option<SemanticsActionRoute> {
        let cell = self.action_cell(ctx)?;
        match ctx.register_local_payload(cell) {
            Ok(target) => {
                let handler: SemanticsActionHandler =
                    Arc::new(move |action, arguments| deliver(target, action, arguments));
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

/// Merges the semantics of all descendants into a single node.
#[derive(Clone, Debug, Default)]
pub struct MergeSemantics {
    child: Child,
}

impl MergeSemantics {
    /// Creates a merge-semantics widget.
    pub fn new() -> Self {
        Self::default()
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
        RenderMergeSemantics::default()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
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
/// The index is zero-based on both sides; the one-based conversion AccessKit's
/// `position_in_set` wants happens once, at the platform boundary.
#[derive(Clone, Debug, Default)]
pub struct IndexedSemantics {
    index: i32,
    child: Child,
}

impl IndexedSemantics {
    /// Creates an indexed-semantics widget for a zero-based `index`.
    #[must_use]
    pub fn new(index: i32) -> Self {
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
